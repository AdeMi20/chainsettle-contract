//! #528 clean-completion rebate, #531 arbiter stake slash, #552 substitute
//! supplier, #580 batch cancel shipments.

#![cfg(test)]

extern crate std;

use super::*;
use crate::test_common::{build_milestones, default_options, setup, single_buyer_vec, TestSetup};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, vec, Address, Env, String, Symbol};

fn sid(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn ipfs(env: &Env) -> Symbol {
    Symbol::new(env, "ipfs")
}

fn proof(env: &Env) -> String {
    String::from_str(env, "QmProof")
}

fn create_standard(t: &TestSetup, client: &ChainSettleContractClient, shipment_id: &String) {
    client.create_shipment(
        shipment_id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000,
        &build_milestones(&t.env),
        &default_options(&t.env),
    );
}

fn confirm_all_milestones(t: &TestSetup, client: &ChainSettleContractClient, id: &String) {
    for i in 0u32..3 {
        client.submit_proof(&t.supplier, id, &i, &proof(&t.env), &ipfs(&t.env));
        client.confirm_milestone(&t.buyer, id, &i);
    }
}

fn milestone_with_deadline(env: &Env, pct: u32, deadline_ledger: u32) -> Milestone {
    Milestone {
        name: String::from_str(env, "M"),
        payment_percent: pct,
        proof_hash: String::from_str(env, ""),
        status: MilestoneStatus::Pending,
        release_after_ledger: 0,
        proof_submitted_ledger: None,
        dispute_opened_ledger: None,
        deadline_ledger,
        penalty_bps_per_ledger: 0,
    }
}

fn open_resolve_appeal_overturn(
    t: &TestSetup,
    client: &ChainSettleContractClient,
    ship_id: &String,
    original_arbiter: &Address,
) {
    client.submit_proof(&t.supplier, ship_id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.raise_dispute(&t.buyer, ship_id, &0u32);
    client.resolve_dispute(original_arbiter, ship_id, &0u32, &true, &None);
    client.appeal_dispute(&t.buyer, ship_id, &0u32);
    let appeal_arbiter = client.get_shipment(ship_id).arbiter;
    client.resolve_dispute(&appeal_arbiter, ship_id, &0u32, &false, &None);
}

// ─── #528 Clean-completion fee rebate ───────────────────────────────────────

#[test]
fn test_set_clean_completion_rebate_bps() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    assert_eq!(client.get_clean_completion_rebate_bps(), 0);
    client.set_clean_completion_rebate_bps(&t.buyer, &2_500u32);
    assert_eq!(client.get_clean_completion_rebate_bps(), 2_500);
}

#[test]
#[should_panic(expected = "bps cannot exceed 10000")]
fn test_clean_completion_rebate_bps_capped() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_clean_completion_rebate_bps(&t.buyer, &10_001u32);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_non_admin_cannot_set_clean_completion_rebate() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_clean_completion_rebate_bps(&t.supplier, &1_000u32);
}

#[test]
fn test_clean_completion_rebate_paid_on_dispute_free_finish() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    // Keep fees in-contract so the rebate can be paid from protocol dust.
    client.set_fee_config(&t.buyer, &100u32, &t.contract_id);
    client.set_clean_completion_rebate_bps(&t.buyer, &5_000u32);

    let id = sid(&t.env, "clean1");
    create_standard(&t, &client, &id);

    let token = token::Client::new(&t.env, &t.token_id);
    let supplier_before = token.balance(&t.supplier);

    confirm_all_milestones(&t, &client, &id);

    let fees_paid = client.get_shipment_fees_paid(&id);
    assert_eq!(fees_paid, 10_000);
    let expected_rebate = fees_paid * 5_000 / 10_000;
    assert_eq!(
        token.balance(&t.supplier) - supplier_before,
        990_000 + expected_rebate
    );
    assert_eq!(client.get_shipment(&id).status, ShipmentStatus::Completed);

    let log = client.get_shipment(&id).audit_log;
    let mut found = false;
    for i in 0..log.len() {
        if log.get(i).unwrap().action == Symbol::new(&t.env, "clean_completion_rebate") {
            found = true;
        }
    }
    assert!(found, "clean_completion_rebate audit entry missing");
}

#[test]
fn test_clean_completion_rebate_skipped_after_dispute() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_fee_config(&t.buyer, &100u32, &t.contract_id);
    client.set_clean_completion_rebate_bps(&t.buyer, &5_000u32);

    let id = sid(&t.env, "disputed1");
    create_standard(&t, &client, &id);

    let token = token::Client::new(&t.env, &t.token_id);
    let supplier_before = token.balance(&t.supplier);

    client.submit_proof(&t.supplier, &id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.raise_dispute(&t.buyer, &id, &0u32);
    client.resolve_dispute(&t.arbiter, &id, &0u32, &true, &None);

    for i in 1u32..3 {
        client.submit_proof(&t.supplier, &id, &i, &proof(&t.env), &ipfs(&t.env));
        client.confirm_milestone(&t.buyer, &id, &i);
    }

    // No rebate: supplier receives nets only (fees stay withheld).
    let supplier_gain = token.balance(&t.supplier) - supplier_before;
    assert!(supplier_gain <= 990_000);
    let log = client.get_shipment(&id).audit_log;
    for i in 0..log.len() {
        assert_ne!(
            log.get(i).unwrap().action,
            Symbol::new(&t.env, "clean_completion_rebate")
        );
    }
}

#[test]
fn test_clean_completion_rebate_skipped_if_treasury_short() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_fee_config(&t.buyer, &100u32, &t.treasury);
    client.set_clean_completion_rebate_bps(&t.buyer, &10_000u32);

    let id = sid(&t.env, "short1");
    create_standard(&t, &client, &id);

    for i in 0u32..2 {
        client.submit_proof(&t.supplier, &id, &i, &proof(&t.env), &ipfs(&t.env));
        client.confirm_milestone(&t.buyer, &id, &i);
    }

    let token = token::Client::new(&t.env, &t.token_id);
    let treasury_bal = token.balance(&t.treasury);
    if treasury_bal > 0 {
        token.transfer(&t.treasury, &t.buyer, &treasury_bal);
    }

    let supplier_before = token.balance(&t.supplier);
    client.submit_proof(&t.supplier, &id, &2u32, &proof(&t.env), &ipfs(&t.env));
    client.confirm_milestone(&t.buyer, &id, &2u32);

    // Final milestone net only (250_000 - 1% fee) — no rebate topped up.
    assert_eq!(token.balance(&t.supplier) - supplier_before, 247_500);
    assert_eq!(client.get_shipment(&id).status, ShipmentStatus::Completed);
}

// ─── #531 Arbiter stake slash ───────────────────────────────────────────────

#[test]
fn test_set_arbiter_slash_bps() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    assert_eq!(client.get_arbiter_slash_bps(), 0);
    client.set_arbiter_slash_bps(&t.buyer, &2_000u32);
    assert_eq!(client.get_arbiter_slash_bps(), 2_000);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_non_admin_cannot_set_arbiter_slash_bps() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.set_arbiter_slash_bps(&t.supplier, &1_000u32);
}

#[test]
fn test_arbiter_stake_slashed_on_overturn_to_buyer() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let arbiter2 = Address::generate(&t.env);
    client.add_arbiter_to_pool(&t.buyer, &t.arbiter);
    client.add_arbiter_to_pool(&t.buyer, &arbiter2);
    client.set_appeal_window_ledgers(&t.buyer, &50u32);
    client.set_arbiter_slash_bps(&t.buyer, &5_000u32);

    let token_admin = token::StellarAssetClient::new(&t.env, &t.token_id);
    token_admin.mint(&t.arbiter, &100_000);
    client.deposit_arbiter_stake(&t.arbiter, &t.token_id, &100_000i128);
    assert_eq!(client.get_arbiter_stake(&t.arbiter), 100_000);

    let token = token::Client::new(&t.env, &t.token_id);
    let ship_id = sid(&t.env, "slash1");
    create_standard(&t, &client, &ship_id);
    let buyer_before = token.balance(&t.buyer);

    open_resolve_appeal_overturn(&t, &client, &ship_id, &t.arbiter);

    assert_eq!(client.get_arbiter_stake(&t.arbiter), 50_000);
    assert!(token.balance(&t.buyer) >= buyer_before + 50_000);
}

#[test]
fn test_arbiter_stake_not_slashed_on_upheld_appeal() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let arbiter2 = Address::generate(&t.env);
    client.add_arbiter_to_pool(&t.buyer, &t.arbiter);
    client.add_arbiter_to_pool(&t.buyer, &arbiter2);
    client.set_appeal_window_ledgers(&t.buyer, &50u32);
    client.set_arbiter_slash_bps(&t.buyer, &5_000u32);

    let token_admin = token::StellarAssetClient::new(&t.env, &t.token_id);
    token_admin.mint(&t.arbiter, &100_000);
    client.deposit_arbiter_stake(&t.arbiter, &t.token_id, &100_000i128);

    let ship_id = sid(&t.env, "uphold1");
    create_standard(&t, &client, &ship_id);
    client.submit_proof(&t.supplier, &ship_id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.raise_dispute(&t.buyer, &ship_id, &0u32);
    client.resolve_dispute(&t.arbiter, &ship_id, &0u32, &true, &None);
    client.appeal_dispute(&t.buyer, &ship_id, &0u32);
    let appeal_arbiter = client.get_shipment(&ship_id).arbiter;
    client.resolve_dispute(&appeal_arbiter, &ship_id, &0u32, &true, &None);

    assert_eq!(client.get_arbiter_stake(&t.arbiter), 100_000);
}

// ─── #552 Substitute supplier ───────────────────────────────────────────────

#[test]
fn test_substitute_supplier_happy_path() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let sub = Address::generate(&t.env);

    let now = t.env.ledger().sequence();
    let id = sid(&t.env, "sub1");
    client.create_shipment(
        &id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000,
        &vec![
            &t.env,
            milestone_with_deadline(&t.env, 50, now + 5),
            milestone_with_deadline(&t.env, 50, now + 100),
        ],
        &default_options(&t.env),
    );

    client.submit_proof(&t.supplier, &id, &0u32, &proof(&t.env), &ipfs(&t.env));
    client.confirm_milestone(&t.buyer, &id, &0u32);

    t.env.ledger().with_mut(|l| l.sequence_number = now + 101);

    client.propose_substitute_supplier(&t.buyer, &id, &1u32, &sub);
    client.approve_substitute_supplier(&t.arbiter, &id, &1u32);
    assert_eq!(client.get_milestone_supplier(&id, &1u32), Some(sub.clone()));

    let token = token::Client::new(&t.env, &t.token_id);
    let sub_before = token.balance(&sub);
    let orig_before = token.balance(&t.supplier);

    client.submit_proof(&sub, &id, &1u32, &proof(&t.env), &ipfs(&t.env));
    client.confirm_milestone(&t.buyer, &id, &1u32);

    assert!(token.balance(&sub) > sub_before);
    assert_eq!(token.balance(&t.supplier), orig_before);
    assert_eq!(client.get_shipment(&id).status, ShipmentStatus::Completed);

    let log = client.get_shipment(&id).audit_log;
    let mut found = false;
    for i in 0..log.len() {
        if log.get(i).unwrap().action == Symbol::new(&t.env, "substitute_approved") {
            found = true;
        }
    }
    assert!(found);
}

#[test]
#[should_panic(expected = "milestone is not overdue")]
fn test_substitute_rejects_non_overdue() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let sub = Address::generate(&t.env);
    let now = t.env.ledger().sequence();
    let id = sid(&t.env, "sub2");
    client.create_shipment(
        &id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000,
        &vec![&t.env, milestone_with_deadline(&t.env, 100, now + 500)],
        &default_options(&t.env),
    );
    client.propose_substitute_supplier(&t.buyer, &id, &0u32, &sub);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_non_arbiter_cannot_approve_substitute() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let sub = Address::generate(&t.env);
    let now = t.env.ledger().sequence();
    let id = sid(&t.env, "sub3");
    client.create_shipment(
        &id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000,
        &vec![&t.env, milestone_with_deadline(&t.env, 100, now + 5)],
        &default_options(&t.env),
    );
    t.env.ledger().with_mut(|l| l.sequence_number = now + 10);
    client.propose_substitute_supplier(&t.buyer, &id, &0u32, &sub);
    client.approve_substitute_supplier(&t.supplier, &id, &0u32);
}

// ─── #580 Batch cancel ──────────────────────────────────────────────────────

#[test]
fn test_batch_cancel_shipments_happy_path() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let id1 = sid(&t.env, "bc1");
    let id2 = sid(&t.env, "bc2");
    create_standard(&t, &client, &id1);
    create_standard(&t, &client, &id2);

    let token = token::Client::new(&t.env, &t.token_id);
    let buyer_before = token.balance(&t.buyer);

    let ids = vec![&t.env, id1.clone(), id2.clone()];
    client.batch_cancel_shipments(&t.buyer, &ids, &String::from_str(&t.env, "winding down"));

    assert_eq!(client.get_shipment(&id1).status, ShipmentStatus::Cancelled);
    assert_eq!(client.get_shipment(&id2).status, ShipmentStatus::Cancelled);
    assert_eq!(token.balance(&t.buyer) - buyer_before, 2_000_000);
}

#[test]
#[should_panic(expected = "shipment not found")]
fn test_batch_cancel_atomic_on_invalid_id() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let id1 = sid(&t.env, "bc3");
    create_standard(&t, &client, &id1);
    let bad = sid(&t.env, "does-not-exist");
    let ids = vec![&t.env, id1, bad];
    client.batch_cancel_shipments(&t.buyer, &ids, &String::from_str(&t.env, "x"));
}

#[test]
#[should_panic(expected = "batch too large")]
fn test_batch_cancel_capped() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut ids = vec![&t.env];
    for i in 0..21 {
        ids.push_back(sid(&t.env, &std::format!("bcx{}", i)));
    }
    client.batch_cancel_shipments(&t.buyer, &ids, &String::from_str(&t.env, "cap"));
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_batch_cancel_requires_buyer() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id1 = sid(&t.env, "bc5");
    create_standard(&t, &client, &id1);
    let ids = vec![&t.env, id1];
    client.batch_cancel_shipments(&t.supplier, &ids, &String::from_str(&t.env, "nope"));
}
