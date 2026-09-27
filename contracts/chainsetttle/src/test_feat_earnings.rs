//! #575 shipment summaries, #576 earnings/spend counters, #577 dispute
//! history, #578 mutual ratings.

#![cfg(test)]

extern crate std;

use super::*;
use crate::test_common::{
    create_standard_shipment, setup, TestSetup,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, BytesN, String, Symbol,
};

fn sid(env: &soroban_sdk::Env, s: &str) -> String {
    String::from_str(env, s)
}

fn proof(env: &soroban_sdk::Env) -> String {
    String::from_str(env, "QmProof")
}

fn ipfs(env: &soroban_sdk::Env) -> Symbol {
    Symbol::new(env, "ipfs")
}

fn create_standard(t: &TestSetup, client: &ChainSettleContractClient, id: &String) {
    create_standard_shipment(
        client,
        &t.env,
        id,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );
}

fn confirm_all(t: &TestSetup, client: &ChainSettleContractClient, id: &String) {
    for i in 0u32..3 {
        client.submit_proof(&t.supplier, id, &i, &proof(&t.env), &ipfs(&t.env));
        client.confirm_milestone(&t.buyer, id, &i);
    }
}

// ── #575 ──────────────────────────────────────────────────────────────────

#[test]
fn test_summary_fields_match_shipment() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id = sid(&t.env, "SUM-1");
    create_standard(&t, &client, &id);

    let full = client.get_shipment(&id);
    let summary = client.get_shipment_summary(&id);

    assert_eq!(summary.id, full.id);
    assert_eq!(summary.status, full.status);
    assert_eq!(summary.buyers, full.buyers);
    assert_eq!(summary.supplier, full.supplier);
    assert_eq!(summary.token, full.token);
    assert_eq!(summary.total_amount, full.total_amount);
    assert_eq!(summary.released_amount, full.released_amount);
    assert_eq!(summary.milestone_count, full.milestones.len());
    assert_eq!(summary.open_disputes, full.open_dispute_count);
    assert_eq!(summary.created_at, full.created_at);
}

#[test]
fn test_summary_batch_skips_unknown_and_enforces_max() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let a = sid(&t.env, "SUM-A");
    let b = sid(&t.env, "SUM-B");
    create_standard(&t, &client, &a);
    create_standard(&t, &client, &b);

    let missing = sid(&t.env, "MISSING");
    let ids = vec![&t.env, a.clone(), missing, b.clone()];
    let summaries = client.get_shipment_summaries(&ids);
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries.get(0).unwrap().id, a);
    assert_eq!(summaries.get(1).unwrap().id, b);

    assert_eq!(client.get_max_summary_batch(), 50);
    client.set_max_summary_batch(&t.buyer, &2);
    assert_eq!(client.get_max_summary_batch(), 2);

    let oversize = vec![&t.env, a.clone(), b.clone(), sid(&t.env, "C")];
    let result = client.try_get_shipment_summaries(&oversize);
    assert!(result.is_err());
}

#[test]
fn test_summary_single_panics_on_unknown() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let result = client.try_get_shipment_summary(&sid(&t.env, "NOPE"));
    assert!(result.is_err());
}

#[test]
fn test_summary_cheaper_than_full_shipment() {
    // Exercises both paths on a shipment with a populated audit log so callers
    // can compare resource cost; summary omits audit_log + milestones.
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id = sid(&t.env, "SUM-COST");
    create_standard(&t, &client, &id);
    confirm_all(&t, &client, &id);

    let full = client.get_shipment(&id);
    let summary = client.get_shipment_summary(&id);
    assert!(full.audit_log.len() > 0);
    assert_eq!(summary.milestone_count, full.milestones.len());
    assert_eq!(summary.status, ShipmentStatus::Completed);
    assert_eq!(summary.released_amount, full.released_amount);
}

// ── #576 ──────────────────────────────────────────────────────────────────

#[test]
fn test_earnings_and_spend_track_releases_net_of_fees() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    // 1% platform fee.
    client.set_fee_config(&t.buyer, &100u32, &t.treasury);

    let id = sid(&t.env, "EARN-1");
    create_standard(&t, &client, &id);
    confirm_all(&t, &client, &id);

    let spend = client.get_buyer_spend(&t.buyer, &t.token_id);
    let earned = client.get_supplier_earnings(&t.supplier, &t.token_id);

    // Gross released from escrow across 25/50/25 = 1_000_000.
    assert_eq!(spend, 1_000_000);
    // Fees excluded from supplier earnings: 1% of 1_000_000 = 10_000.
    assert_eq!(earned, 990_000);
}

#[test]
fn test_refunds_do_not_count_as_spend_or_earnings() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_fee_config(&t.buyer, &100u32, &t.treasury);

    let id = sid(&t.env, "EARN-REFUND");
    create_standard(&t, &client, &id);

    client.submit_proof(&t.supplier, &id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.raise_dispute(&t.buyer, &id, &0u32);
    // Reject → buyer-favor; no supplier payout on this path.
    client.resolve_dispute(&t.arbiter, &id, &0u32, &false, &None);

    assert_eq!(client.get_buyer_spend(&t.buyer, &t.token_id), 0);
    assert_eq!(client.get_supplier_earnings(&t.supplier, &t.token_id), 0);
}

// ── #577 ──────────────────────────────────────────────────────────────────

#[test]
fn test_dispute_history_both_parties_and_outcomes() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id = sid(&t.env, "DISP-HIST");
    create_standard(&t, &client, &id);

    client.submit_proof(&t.supplier, &id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.raise_dispute(&t.buyer, &id, &0u32);
    let opened = t.env.ledger().sequence();
    client.resolve_dispute(&t.arbiter, &id, &0u32, &true, &None);

    let buyer_hist = client.get_dispute_history(&t.buyer, &None, &10);
    let supplier_hist = client.get_dispute_history(&t.supplier, &None, &10);
    assert_eq!(buyer_hist.len(), 1);
    assert_eq!(supplier_hist.len(), 1);

    let rec = buyer_hist.get(0).unwrap();
    assert_eq!(rec.shipment_id, id);
    assert_eq!(rec.milestone_index, 0);
    assert_eq!(rec.opened_ledger, opened);
    assert_eq!(rec.outcome, DisputeOutcome::Supplier);
    assert_eq!(supplier_hist.get(0).unwrap().outcome, DisputeOutcome::Supplier);
}

#[test]
fn test_withdrawn_dispute_recorded() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id = sid(&t.env, "DISP-WD");
    create_standard(&t, &client, &id);

    client.submit_proof(&t.supplier, &id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.raise_dispute(&t.buyer, &id, &0u32);
    client.withdraw_dispute(&t.buyer, &id, &0u32);

    let hist = client.get_dispute_history(&t.buyer, &None, &10);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist.get(0).unwrap().outcome, DisputeOutcome::Withdrawn);
    assert_eq!(
        client.get_dispute_history(&t.supplier, &None, &10).get(0).unwrap().outcome,
        DisputeOutcome::Withdrawn
    );
}

#[test]
fn test_dispute_history_pagination_stable() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    for n in 0..3 {
        let id = sid(&t.env, &std::format!("DISP-PAGE-{}", n));
        create_standard(&t, &client, &id);
        client.submit_proof(&t.supplier, &id, &0u32, &proof(&t.env), &ipfs(&t.env));
        client.raise_dispute(&t.buyer, &id, &0u32);
        client.resolve_dispute(&t.arbiter, &id, &0u32, &true, &None);
    }

    let page1 = client.get_dispute_history(&t.buyer, &None, &2);
    assert_eq!(page1.len(), 2);
    let page2 = client.get_dispute_history(&t.buyer, &Some(2), &2);
    assert_eq!(page2.len(), 1);
    assert_ne!(page1.get(0).unwrap().shipment_id, page2.get(0).unwrap().shipment_id);
}

// ── #578 ──────────────────────────────────────────────────────────────────

#[test]
fn test_rate_counterparty_happy_path_and_averages() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_rating_window_ledgers(&t.buyer, &100);

    let id = sid(&t.env, "RATE-1");
    create_standard(&t, &client, &id);
    confirm_all(&t, &client, &id);

    client.rate_counterparty(&t.buyer, &id, &5u32, &None);
    client.rate_counterparty(&t.supplier, &id, &4u32, &None);

    // Buyer rated supplier 5 → supplier summary (1, 500).
    let (scount, savg) = client.get_rating_summary(&t.supplier);
    assert_eq!(scount, 1);
    assert_eq!(savg, 500);

    // Supplier rated buyer 4 → buyer summary (1, 400).
    let (bcount, bavg) = client.get_rating_summary(&t.buyer);
    assert_eq!(bcount, 1);
    assert_eq!(bavg, 400);
}

#[test]
fn test_rate_rejects_bad_stars_double_rate_and_window() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_rating_window_ledgers(&t.buyer, &10);

    let id = sid(&t.env, "RATE-BAD");
    create_standard(&t, &client, &id);

    // Not completed yet.
    assert!(client.try_rate_counterparty(&t.buyer, &id, &5u32, &None).is_err());

    confirm_all(&t, &client, &id);

    assert!(client.try_rate_counterparty(&t.buyer, &id, &0u32, &None).is_err());
    assert!(client.try_rate_counterparty(&t.buyer, &id, &6u32, &None).is_err());

    let hash = BytesN::from_array(&t.env, &[7u8; 32]);
    client.rate_counterparty(&t.buyer, &id, &3u32, &Some(hash));
    assert!(client.try_rate_counterparty(&t.buyer, &id, &4u32, &None).is_err());

    // Advance past the rating window.
    t.env.ledger().with_mut(|li| {
        li.sequence_number += 20;
    });
    assert!(client.try_rate_counterparty(&t.supplier, &id, &5u32, &None).is_err());
}

#[test]
fn test_ratings_disabled_when_window_zero() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    // Default window is 0 = disabled.
    assert_eq!(client.get_rating_window_ledgers(), 0);

    let id = sid(&t.env, "RATE-OFF");
    create_standard(&t, &client, &id);
    confirm_all(&t, &client, &id);
    assert!(client.try_rate_counterparty(&t.buyer, &id, &5u32, &None).is_err());
}

#[test]
fn test_unauthorized_rater_rejected() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_rating_window_ledgers(&t.buyer, &50);

    let id = sid(&t.env, "RATE-UNAUTH");
    create_standard(&t, &client, &id);
    confirm_all(&t, &client, &id);

    let stranger = Address::generate(&t.env);
    assert!(client.try_rate_counterparty(&stranger, &id, &5u32, &None).is_err());
}
