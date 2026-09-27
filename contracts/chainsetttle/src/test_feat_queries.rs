//! #571–#574: version queries, arbiter/logistics indexes, upcoming deadlines.

#![cfg(test)]

extern crate std;

use super::*;
use crate::test_common::{
    build_milestones, create_standard_shipment, default_options, setup, single_buyer_vec,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, String, Symbol,
};

#[test]
fn test_version_matches_cargo_constants() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    assert_eq!(
        client.version(),
        (
            constants::VERSION_MAJOR,
            constants::VERSION_MINOR,
            constants::VERSION_PATCH
        )
    );
    assert_eq!(client.version(), (0, 1, 0));
}

#[test]
fn test_storage_schema_version_updates_after_migrate() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    assert_eq!(client.storage_schema_version(), 0);
    client.migrate();
    assert_eq!(
        client.storage_schema_version(),
        constants::STORAGE_SCHEMA_VERSION
    );
    // Idempotent: calling again keeps the same schema version.
    client.migrate();
    assert_eq!(
        client.storage_schema_version(),
        constants::STORAGE_SCHEMA_VERSION
    );
}

#[test]
fn test_get_shipments_by_arbiter_indexes_on_create_and_paginates() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let other_arbiter = Address::generate(&t.env);
    let ship_a = String::from_str(&t.env, "ARB-IDX-1");
    let ship_b = String::from_str(&t.env, "ARB-IDX-2");
    let ship_c = String::from_str(&t.env, "ARB-IDX-3");

    create_standard_shipment(
        &client,
        &t.env,
        &ship_a,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );
    create_standard_shipment(
        &client,
        &t.env,
        &ship_b,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );
    create_standard_shipment(
        &client,
        &t.env,
        &ship_c,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &other_arbiter,
        &t.token_id,
        1_000_000,
    );

    let page1 = client.get_shipments_by_arbiter(&t.arbiter, &None, &1);
    assert_eq!(page1.len(), 1);
    assert_eq!(page1.get(0).unwrap(), ship_a);

    let page2 = client.get_shipments_by_arbiter(&t.arbiter, &Some(1), &1);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2.get(0).unwrap(), ship_b);

    let all = client.get_shipments_by_arbiter(&t.arbiter, &None, &50);
    assert_eq!(all.len(), 2);

    let other = client.get_shipments_by_arbiter(&other_arbiter, &None, &50);
    assert_eq!(other.len(), 1);
    assert_eq!(other.get(0).unwrap(), ship_c);

    let empty = client.get_shipments_by_arbiter(&Address::generate(&t.env), &None, &10);
    assert_eq!(empty.len(), 0);
}

#[test]
fn test_arbiter_index_updates_on_rotation() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let shipment_id = String::from_str(&t.env, "ARB-ROT-1");
    let new_arbiter = Address::generate(&t.env);

    create_standard_shipment(
        &client,
        &t.env,
        &shipment_id,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );

    assert_eq!(
        client
            .get_shipments_by_arbiter(&t.arbiter, &None, &50)
            .len(),
        1
    );

    client.propose_arbiter_rotation(&t.buyer, &shipment_id, &new_arbiter);
    client.propose_arbiter_rotation(&t.supplier, &shipment_id, &new_arbiter);

    let old_list = client.get_shipments_by_arbiter(&t.arbiter, &None, &50);
    assert_eq!(old_list.len(), 0);

    let new_list = client.get_shipments_by_arbiter(&new_arbiter, &None, &50);
    assert_eq!(new_list.len(), 1);
    assert_eq!(new_list.get(0).unwrap(), shipment_id);

    assert_eq!(client.get_shipment(&shipment_id).arbiter, new_arbiter);
}

#[test]
fn test_arbiter_index_updates_on_appeal_reassignment() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let shipment_id = String::from_str(&t.env, "ARB-APL-1");
    let pool_arb1 = Address::generate(&t.env);
    let pool_arb2 = Address::generate(&t.env);

    client.add_arbiter_to_pool(&t.buyer, &pool_arb1);
    client.add_arbiter_to_pool(&t.buyer, &pool_arb2);
    client.set_appeal_window_ledgers(&t.buyer, &50u32);

    create_standard_shipment(
        &client,
        &t.env,
        &shipment_id,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );

    client.submit_proof(
        &t.supplier,
        &shipment_id,
        &0,
        &String::from_str(&t.env, "ipfs://proof"),
        &Symbol::new(&t.env, "ipfs"),
    );
    client.raise_dispute(&t.buyer, &shipment_id, &0);

    let original = client.get_shipment(&shipment_id).arbiter.clone();
    client.resolve_dispute(&original, &shipment_id, &0, &true, &None);

    assert!(client
        .get_shipments_by_arbiter(&original, &None, &50)
        .contains(shipment_id.clone()));

    client.appeal_dispute(&t.buyer, &shipment_id, &0);

    assert!(!client
        .get_shipments_by_arbiter(&original, &None, &50)
        .contains(shipment_id.clone()));

    let new_arbiter = client.get_shipment(&shipment_id).arbiter;
    assert_ne!(new_arbiter, original);
    assert!(client
        .get_shipments_by_arbiter(&new_arbiter, &None, &50)
        .contains(shipment_id.clone()));
}

#[test]
fn test_get_shipments_by_logistics_indexes_and_paginates() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let other_logistics = Address::generate(&t.env);
    let ship_a = String::from_str(&t.env, "LOG-IDX-1");
    let ship_b = String::from_str(&t.env, "LOG-IDX-2");
    let ship_c = String::from_str(&t.env, "LOG-IDX-3");

    create_standard_shipment(
        &client,
        &t.env,
        &ship_a,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );
    create_standard_shipment(
        &client,
        &t.env,
        &ship_b,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );
    create_standard_shipment(
        &client,
        &t.env,
        &ship_c,
        &t.buyer,
        &t.supplier,
        &other_logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );

    let page1 = client.get_shipments_by_logistics(&t.logistics, &None, &1);
    assert_eq!(page1.len(), 1);
    assert_eq!(page1.get(0).unwrap(), ship_a);

    let page2 = client.get_shipments_by_logistics(&t.logistics, &Some(1), &10);
    assert_eq!(page2.len(), 1);
    assert_eq!(page2.get(0).unwrap(), ship_b);

    let other = client.get_shipments_by_logistics(&other_logistics, &None, &50);
    assert_eq!(other.len(), 1);
    assert_eq!(other.get(0).unwrap(), ship_c);
}

#[test]
fn test_logistics_index_updates_on_transfer() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let shipment_id = String::from_str(&t.env, "LOG-XFER-1");
    let new_logistics = Address::generate(&t.env);

    create_standard_shipment(
        &client,
        &t.env,
        &shipment_id,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );

    assert_eq!(
        client
            .get_shipments_by_logistics(&t.logistics, &None, &50)
            .len(),
        1
    );

    client.transfer_logistics(&t.logistics, &shipment_id, &new_logistics);

    assert_eq!(
        client
            .get_shipments_by_logistics(&t.logistics, &None, &50)
            .len(),
        0
    );
    let new_list = client.get_shipments_by_logistics(&new_logistics, &None, &50);
    assert_eq!(new_list.len(), 1);
    assert_eq!(new_list.get(0).unwrap(), shipment_id);
    assert_eq!(client.get_shipment(&shipment_id).logistics, new_logistics);
}

#[test]
fn test_transfer_logistics_rejects_unauthorized() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let shipment_id = String::from_str(&t.env, "LOG-UNAUTH");
    let impostor = Address::generate(&t.env);
    let new_logistics = Address::generate(&t.env);

    create_standard_shipment(
        &client,
        &t.env,
        &shipment_id,
        &t.buyer,
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        1_000_000,
    );

    let result = client.try_transfer_logistics(&impostor, &shipment_id, &new_logistics);
    assert!(result.is_err());
}

#[test]
fn test_get_upcoming_deadlines_sorted_filtered_and_capped() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    t.env.ledger().set_sequence_number(100);

    let ship_a = String::from_str(&t.env, "DL-A");
    let ship_b = String::from_str(&t.env, "DL-B");

    let mut milestones_a = build_milestones(&t.env);
    let mut m0 = milestones_a.get(0).unwrap();
    m0.deadline_ledger = 150; // inside window
    milestones_a.set(0, m0);
    let mut m1 = milestones_a.get(1).unwrap();
    m1.deadline_ledger = 130; // sooner
    milestones_a.set(1, m1);
    let mut m2 = milestones_a.get(2).unwrap();
    m2.deadline_ledger = 500; // outside window
    milestones_a.set(2, m2);

    client.create_shipment(
        &ship_a,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &milestones_a,
        &default_options(&t.env),
    );

    let mut milestones_b = build_milestones(&t.env);
    let mut b0 = milestones_b.get(0).unwrap();
    b0.deadline_ledger = 140;
    milestones_b.set(0, b0);

    client.create_shipment(
        &ship_b,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &milestones_b,
        &default_options(&t.env),
    );

    // within_ledgers=100 → window (100, 200]
    let results = client.get_upcoming_deadlines(&t.buyer, &100u32, &10u32);
    assert_eq!(results.len(), 3);
    // Sorted ascending by deadline: 130, 140, 150
    assert_eq!(results.get(0).unwrap(), (ship_a.clone(), 1u32, 130u32));
    assert_eq!(results.get(1).unwrap(), (ship_b.clone(), 0u32, 140u32));
    assert_eq!(results.get(2).unwrap(), (ship_a.clone(), 0u32, 150u32));

    let capped = client.get_upcoming_deadlines(&t.buyer, &100u32, &1u32);
    assert_eq!(capped.len(), 1);
    assert_eq!(capped.get(0).unwrap().2, 130u32);

    // Supplier sees the same active deadlines.
    let as_supplier = client.get_upcoming_deadlines(&t.supplier, &100u32, &50u32);
    assert_eq!(as_supplier.len(), 3);

    // Unrelated address → empty.
    let stranger = client.get_upcoming_deadlines(&Address::generate(&t.env), &100u32, &50u32);
    assert_eq!(stranger.len(), 0);

    // Zero window / limit → empty.
    assert_eq!(client.get_upcoming_deadlines(&t.buyer, &0u32, &10u32).len(), 0);
    assert_eq!(client.get_upcoming_deadlines(&t.buyer, &100u32, &0u32).len(), 0);
}

#[test]
fn test_upcoming_deadlines_excludes_completed_and_cancelled() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    t.env.ledger().set_sequence_number(50);

    let shipment_id = String::from_str(&t.env, "DL-CANCEL");
    let mut milestones = build_milestones(&t.env);
    let mut m0 = milestones.get(0).unwrap();
    m0.deadline_ledger = 80;
    milestones.set(0, m0);

    client.create_shipment(
        &shipment_id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &milestones,
        &default_options(&t.env),
    );

    assert_eq!(
        client.get_upcoming_deadlines(&t.buyer, &100u32, &10u32).len(),
        1
    );

    client.cancel_shipment(&t.buyer, &shipment_id);

    assert_eq!(
        client.get_upcoming_deadlines(&t.buyer, &100u32, &10u32).len(),
        0
    );
}
