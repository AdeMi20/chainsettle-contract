#![cfg(test)]

//! Tests for #543–#546:
//! - Independent inspector sign-off
//! - Per-milestone designated proof submitter
//! - Dual attestation of proof
//! - Oracle-triggered condition breach dispute

extern crate std;

use super::*;
use crate::test_common::{default_options, setup, single_buyer_vec, TestSetup};
use soroban_sdk::{
    testutils::{Address as _, Events},
    vec, Address, BytesN, String, Symbol, Vec,
};

fn proof_hash(env: &soroban_sdk::Env) -> String {
    String::from_str(env, "ipfs://proof")
}

fn proof_type(env: &soroban_sdk::Env) -> Symbol {
    Symbol::new(env, "ipfs")
}

fn single_milestone(env: &soroban_sdk::Env) -> Vec<Milestone> {
    vec![
        env,
        Milestone {
            name: String::from_str(env, "All"),
            payment_percent: 100,
            proof_hash: String::from_str(env, ""),
            status: MilestoneStatus::Pending,
            release_after_ledger: 0,
            proof_submitted_ledger: None,
            dispute_opened_ledger: None,
            deadline_ledger: 0,
            penalty_bps_per_ledger: 0,
        },
    ]
}

fn two_milestones(env: &soroban_sdk::Env) -> Vec<Milestone> {
    vec![
        env,
        Milestone {
            name: String::from_str(env, "A"),
            payment_percent: 50,
            proof_hash: String::from_str(env, ""),
            status: MilestoneStatus::Pending,
            release_after_ledger: 0,
            proof_submitted_ledger: None,
            dispute_opened_ledger: None,
            deadline_ledger: 0,
            penalty_bps_per_ledger: 0,
        },
        Milestone {
            name: String::from_str(env, "B"),
            payment_percent: 50,
            proof_hash: String::from_str(env, ""),
            status: MilestoneStatus::Pending,
            release_after_ledger: 0,
            proof_submitted_ledger: None,
            dispute_opened_ledger: None,
            deadline_ledger: 0,
            penalty_bps_per_ledger: 0,
        },
    ]
}

fn create_with_opts(t: &TestSetup, id: &str, opts: &ShipmentOptions, milestones: Vec<Milestone>) -> String {
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let ship_id = String::from_str(&t.env, id);
    client.create_shipment(
        &ship_id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000,
        &milestones,
        opts,
    );
    ship_id
}

fn hash32(env: &soroban_sdk::Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

// ============================================================
// #543 — Inspector sign-off
// ============================================================

#[test]
fn test_inspector_sign_off_happy_path() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let inspector = Address::generate(&t.env);
    let mut opts = default_options(&t.env);
    opts.inspector = Some(inspector.clone());
    opts.inspected_milestones = vec![&t.env, 0u32];

    let id = create_with_opts(&t, "insp-ok", &opts, single_milestone(&t.env));
    client.submit_proof(
        &t.supplier,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    let report = hash32(&t.env, 7);
    client.inspector_sign_off(&inspector, &id, &0u32, &report);

    let rec = client.get_inspection(&id, &0u32).unwrap();
    assert_eq!(rec.inspector, inspector);
    assert_eq!(rec.report_hash, report);

    client.confirm_milestone(&t.buyer, &id, &0u32);
    assert_eq!(
        client.get_milestone(&id, &0u32).status,
        MilestoneStatus::Confirmed
    );
}

#[test]
#[should_panic(expected = "inspector sign-off required")]
fn test_confirm_blocked_without_sign_off() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let inspector = Address::generate(&t.env);
    let mut opts = default_options(&t.env);
    opts.inspector = Some(inspector);
    opts.inspected_milestones = vec![&t.env, 0u32];

    let id = create_with_opts(&t, "insp-block", &opts, single_milestone(&t.env));
    client.submit_proof(
        &t.supplier,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    client.confirm_milestone(&t.buyer, &id, &0u32);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_inspector_sign_off_wrong_caller() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let inspector = Address::generate(&t.env);
    let mut opts = default_options(&t.env);
    opts.inspector = Some(inspector);
    opts.inspected_milestones = vec![&t.env, 0u32];

    let id = create_with_opts(&t, "insp-auth", &opts, single_milestone(&t.env));
    client.inspector_sign_off(&t.supplier, &id, &0u32, &hash32(&t.env, 1));
}

#[test]
fn test_non_inspected_milestone_unaffected() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let inspector = Address::generate(&t.env);
    let mut opts = default_options(&t.env);
    opts.inspector = Some(inspector);
    // Only milestone 0 requires inspection; milestone 1 does not.
    opts.inspected_milestones = vec![&t.env, 0u32];

    let id = create_with_opts(&t, "insp-partial", &opts, two_milestones(&t.env));
    client.submit_proof(
        &t.supplier,
        &id,
        &1u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    client.confirm_milestone(&t.buyer, &id, &1u32);
    assert_eq!(
        client.get_milestone(&id, &1u32).status,
        MilestoneStatus::Confirmed
    );
}

// ============================================================
// #544 — Designated proof submitter
// ============================================================

#[test]
fn test_designated_logistics_can_submit() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    opts.proof_submitters = vec![&t.env, t.logistics.clone()];

    let id = create_with_opts(&t, "ps-log", &opts, single_milestone(&t.env));
    assert_eq!(client.get_proof_submitter(&id, &0u32), t.logistics);

    client.submit_proof(
        &t.logistics,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    assert_eq!(
        client.get_milestone(&id, &0u32).status,
        MilestoneStatus::ProofSubmitted
    );
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_designated_rejects_supplier() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    opts.proof_submitters = vec![&t.env, t.logistics.clone()];

    let id = create_with_opts(&t, "ps-rej", &opts, single_milestone(&t.env));
    client.submit_proof(
        &t.supplier,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
}

#[test]
fn test_default_proof_submitter_is_supplier() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id = create_with_opts(
        &t,
        "ps-def",
        &default_options(&t.env),
        single_milestone(&t.env),
    );
    assert_eq!(client.get_proof_submitter(&id, &0u32), t.supplier);
}

#[test]
#[should_panic(expected = "proof_submitters length must match milestone count")]
fn test_proof_submitters_length_mismatch() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    // Two milestones but only one submitter entry.
    opts.proof_submitters = vec![&t.env, t.logistics.clone()];
    let _ = create_with_opts(&t, "ps-len", &opts, two_milestones(&t.env));
    let _ = client;
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_correct_proof_follows_designated_rule() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    opts.proof_submitters = vec![&t.env, t.logistics.clone()];

    let id = create_with_opts(&t, "ps-corr", &opts, single_milestone(&t.env));
    client.submit_proof(
        &t.logistics,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    // Supplier is not the designated submitter.
    client.correct_proof(
        &t.supplier,
        &id,
        &0u32,
        &String::from_str(&t.env, "ipfs://fixed"),
        &proof_type(&t.env),
    );
}

// ============================================================
// #545 — Dual attestation
// ============================================================

#[test]
fn test_dual_attestation_happy_path() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    opts.require_dual_attestation = true;
    // Allow supplier to submit first; logistics attests second.
    opts.proof_submitters = vec![&t.env, t.supplier.clone()];

    let id = create_with_opts(&t, "dual-ok", &opts, single_milestone(&t.env));
    let hash = proof_hash(&t.env);
    client.submit_proof(&t.supplier, &id, &0u32, &hash, &proof_type(&t.env));

    // Still pending after a single attestation.
    assert_eq!(
        client.get_milestone(&id, &0u32).status,
        MilestoneStatus::Pending
    );

    client.attest_proof(&t.logistics, &id, &0u32, &hash);
    let m = client.get_milestone(&id, &0u32);
    assert_eq!(m.status, MilestoneStatus::ProofSubmitted);
    assert_eq!(m.proof_hash, hash);
    assert!(m.proof_submitted_ledger.is_some());
}

#[test]
#[should_panic(expected = "proof hash mismatch")]
fn test_dual_attestation_hash_mismatch() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    opts.require_dual_attestation = true;
    opts.proof_submitters = vec![&t.env, t.supplier.clone()];

    let id = create_with_opts(&t, "dual-mis", &opts, single_milestone(&t.env));
    client.submit_proof(
        &t.supplier,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    client.attest_proof(
        &t.logistics,
        &id,
        &0u32,
        &String::from_str(&t.env, "ipfs://other"),
    );
}

#[test]
fn test_dual_disabled_unchanged() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let id = create_with_opts(
        &t,
        "dual-off",
        &default_options(&t.env),
        single_milestone(&t.env),
    );
    client.submit_proof(
        &t.supplier,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );
    assert_eq!(
        client.get_milestone(&id, &0u32).status,
        MilestoneStatus::ProofSubmitted
    );
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_dual_attest_rejects_outsider() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let mut opts = default_options(&t.env);
    opts.require_dual_attestation = true;
    opts.proof_submitters = vec![&t.env, t.supplier.clone()];
    let outsider = Address::generate(&t.env);

    let id = create_with_opts(&t, "dual-out", &opts, single_milestone(&t.env));
    let hash = proof_hash(&t.env);
    client.submit_proof(&t.supplier, &id, &0u32, &hash, &proof_type(&t.env));
    client.attest_proof(&outsider, &id, &0u32, &hash);
}

// ============================================================
// #546 — Oracle condition breach
// ============================================================

#[test]
fn test_condition_breach_opens_dispute_at_threshold() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let o1 = Address::generate(&t.env);
    let o2 = Address::generate(&t.env);
    let o3 = Address::generate(&t.env);

    let purpose = Symbol::new(&t.env, "coldchain");
    let mut members = Vec::new(&t.env);
    members.push_back(o1.clone());
    members.push_back(o2.clone());
    members.push_back(o3.clone());
    // Admin in test_common setup is `buyer`.
    client.register_oracle_group(&t.buyer, &purpose, &members, &2u32);

    let id = create_with_opts(
        &t,
        "breach-ok",
        &default_options(&t.env),
        single_milestone(&t.env),
    );
    client.set_shipment_oracle_purpose(&t.buyer, &id, &purpose);

    client.submit_proof(
        &t.supplier,
        &id,
        &0u32,
        &proof_hash(&t.env),
        &proof_type(&t.env),
    );

    // First report — below threshold, no dispute yet.
    client.report_condition_breach(&o1, &id, &0u32, &hash32(&t.env, 1));
    assert_eq!(
        client.get_milestone(&id, &0u32).status,
        MilestoneStatus::ProofSubmitted
    );
    assert_eq!(client.get_shipment(&id).open_dispute_count, 0);

    // Duplicate from o1 ignored.
    client.report_condition_breach(&o1, &id, &0u32, &hash32(&t.env, 9));
    assert_eq!(client.get_shipment(&id).open_dispute_count, 0);

    // Second distinct report reaches threshold → dispute opens.
    client.report_condition_breach(&o2, &id, &0u32, &hash32(&t.env, 2));
    assert_eq!(
        client.get_milestone(&id, &0u32).status,
        MilestoneStatus::Disputed
    );
    assert_eq!(client.get_shipment(&id).open_dispute_count, 1);

    // Works alongside N-of-M attestation (group still registered / queryable).
    assert_eq!(client.get_oracle_attestation_count(&id, &0u32), 0);
    let _ = client.get_oracle_group(&purpose);
}

#[test]
#[should_panic(expected = "caller is not a member of the assigned oracle group")]
fn test_condition_breach_rejects_non_member() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let o1 = Address::generate(&t.env);
    let outsider = Address::generate(&t.env);
    let purpose = Symbol::new(&t.env, "coldchain");
    let mut members = Vec::new(&t.env);
    members.push_back(o1.clone());
    client.register_oracle_group(&t.buyer, &purpose, &members, &1u32);

    let id = create_with_opts(
        &t,
        "breach-auth",
        &default_options(&t.env),
        single_milestone(&t.env),
    );
    client.set_shipment_oracle_purpose(&t.buyer, &id, &purpose);
    client.report_condition_breach(&outsider, &id, &0u32, &hash32(&t.env, 1));
}

#[test]
fn test_condition_breach_event_emitted() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let o1 = Address::generate(&t.env);
    let purpose = Symbol::new(&t.env, "hazmat");
    let mut members = Vec::new(&t.env);
    members.push_back(o1.clone());
    client.register_oracle_group(&t.buyer, &purpose, &members, &1u32);

    let id = create_with_opts(
        &t,
        "breach-evt",
        &default_options(&t.env),
        single_milestone(&t.env),
    );
    client.set_shipment_oracle_purpose(&t.buyer, &id, &purpose);
    let before = t.env.events().all().len();
    client.report_condition_breach(&o1, &id, &0u32, &hash32(&t.env, 3));
    assert!(t.env.events().all().len() > before);
}
