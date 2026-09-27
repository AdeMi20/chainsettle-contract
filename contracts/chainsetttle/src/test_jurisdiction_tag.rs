// #385 — Jurisdiction/compliance tag per shipment for regulatory filtering.
//
// Verifies the optional `jurisdiction` field set at shipment creation is
// stored immutably on the shipment record and indexed for off-chain
// compliance tooling via `get_shipments_by_jurisdiction`.

#![cfg(test)]

extern crate std;

use super::*;
use crate::test_common::{build_milestones, default_options, setup, single_buyer_vec};
use soroban_sdk::{testutils::Address as _, String, Symbol};

#[test]
fn test_shipment_untagged_by_default() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let shipment_id = String::from_str(&t.env, "JUR-001");
    client.create_shipment(
        &shipment_id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &build_milestones(&t.env),
        &default_options(&t.env),
    );

    assert_eq!(client.get_shipment_jurisdiction(&shipment_id), None);
}

#[test]
fn test_shipment_tagged_with_jurisdiction_at_creation() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let shipment_id = String::from_str(&t.env, "JUR-002");
    let mut opts = default_options(&t.env);
    let us = Symbol::new(&t.env, "US");
    opts.jurisdiction = Some(us.clone());

    client.create_shipment(
        &shipment_id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &build_milestones(&t.env),
        &opts,
    );

    assert_eq!(client.get_shipment_jurisdiction(&shipment_id), Some(us));
}

#[test]
fn test_get_shipments_by_jurisdiction_filters_correctly() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let us = Symbol::new(&t.env, "US");
    let eu = Symbol::new(&t.env, "EU");

    let mut us_opts = default_options(&t.env);
    us_opts.jurisdiction = Some(us.clone());
    let mut eu_opts = default_options(&t.env);
    eu_opts.jurisdiction = Some(eu.clone());

    let ship_us_1 = String::from_str(&t.env, "JUR-US-1");
    let ship_us_2 = String::from_str(&t.env, "JUR-US-2");
    let ship_eu_1 = String::from_str(&t.env, "JUR-EU-1");

    for (id, opts) in [
        (&ship_us_1, &us_opts),
        (&ship_us_2, &us_opts),
        (&ship_eu_1, &eu_opts),
    ] {
        client.create_shipment(
            id,
            &single_buyer_vec(&t.env, &t.buyer),
            &t.supplier,
            &t.logistics,
            &t.arbiter,
            &t.token_id,
            &1_000_000i128,
            &build_milestones(&t.env),
            opts,
        );
    }

    let us_shipments = client.get_shipments_by_jurisdiction(&us);
    assert_eq!(us_shipments.len(), 2);
    assert!(us_shipments.contains(ship_us_1.clone()));
    assert!(us_shipments.contains(ship_us_2.clone()));

    let eu_shipments = client.get_shipments_by_jurisdiction(&eu);
    assert_eq!(eu_shipments.len(), 1);
    assert!(eu_shipments.contains(ship_eu_1.clone()));
}

#[test]
fn test_get_shipments_by_jurisdiction_empty_for_unused_tag() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let unused = Symbol::new(&t.env, "APAC");
    let result = client.get_shipments_by_jurisdiction(&unused);
    assert_eq!(result.len(), 0);
}

fn create_tagged_shipment(
    t: &crate::test_common::TestSetup,
    buyer: &Address,
    id: &str,
    jurisdiction: Option<Symbol>,
) -> String {
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let shipment_id = String::from_str(&t.env, id);
    let mut opts = default_options(&t.env);
    opts.jurisdiction = jurisdiction;
    client.create_shipment(
        &shipment_id,
        &single_buyer_vec(&t.env, buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &build_milestones(&t.env),
        &opts,
    );
    shipment_id
}

#[test]
fn test_get_shipment_jurisdiction_none_for_unknown_shipment() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let missing = String::from_str(&t.env, "JUR-MISSING");
    assert_eq!(client.get_shipment_jurisdiction(&missing), None);
}

#[test]
fn test_untagged_shipments_not_indexed_alongside_tagged() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let us = Symbol::new(&t.env, "US");
    let tagged = create_tagged_shipment(&t, &t.buyer, "JUR-MIX-1", Some(us.clone()));
    let untagged = create_tagged_shipment(&t, &t.buyer, "JUR-MIX-2", None);

    let us_shipments = client.get_shipments_by_jurisdiction(&us);
    assert_eq!(us_shipments.len(), 1);
    assert!(us_shipments.contains(tagged));
    assert!(!us_shipments.contains(untagged.clone()));
    assert_eq!(client.get_shipment_jurisdiction(&untagged), None);
}

#[test]
fn test_jurisdiction_tags_are_case_sensitive() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let upper = Symbol::new(&t.env, "US");
    let lower = Symbol::new(&t.env, "us");
    let ship_upper = create_tagged_shipment(&t, &t.buyer, "JUR-CASE-1", Some(upper.clone()));
    let ship_lower = create_tagged_shipment(&t, &t.buyer, "JUR-CASE-2", Some(lower.clone()));

    let upper_shipments = client.get_shipments_by_jurisdiction(&upper);
    assert_eq!(upper_shipments.len(), 1);
    assert!(upper_shipments.contains(ship_upper));

    let lower_shipments = client.get_shipments_by_jurisdiction(&lower);
    assert_eq!(lower_shipments.len(), 1);
    assert!(lower_shipments.contains(ship_lower));
}

#[test]
fn test_jurisdiction_index_preserves_creation_order() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let eu = Symbol::new(&t.env, "EU_MIFID");
    let first = create_tagged_shipment(&t, &t.buyer, "JUR-ORD-1", Some(eu.clone()));
    let second = create_tagged_shipment(&t, &t.buyer, "JUR-ORD-2", Some(eu.clone()));
    let third = create_tagged_shipment(&t, &t.buyer, "JUR-ORD-3", Some(eu.clone()));

    let eu_shipments = client.get_shipments_by_jurisdiction(&eu);
    assert_eq!(eu_shipments.len(), 3);
    assert_eq!(eu_shipments.get(0).unwrap(), first);
    assert_eq!(eu_shipments.get(1).unwrap(), second);
    assert_eq!(eu_shipments.get(2).unwrap(), third);
}

#[test]
fn test_jurisdiction_index_spans_multiple_buyers() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let uk = Symbol::new(&t.env, "UK");
    let from_buyer = create_tagged_shipment(&t, &t.buyer, "JUR-BUY-1", Some(uk.clone()));
    let from_buyer2 = create_tagged_shipment(&t, &t.buyer2, "JUR-BUY-2", Some(uk.clone()));

    let uk_shipments = client.get_shipments_by_jurisdiction(&uk);
    assert_eq!(uk_shipments.len(), 2);
    assert!(uk_shipments.contains(from_buyer));
    assert!(uk_shipments.contains(from_buyer2));
}

#[test]
fn test_jurisdiction_tag_persists_after_cancellation() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let us = Symbol::new(&t.env, "US");
    let shipment_id = create_tagged_shipment(&t, &t.buyer, "JUR-CXL-1", Some(us.clone()));

    client.cancel_shipment(&t.buyer, &shipment_id);

    assert_eq!(client.get_shipment_jurisdiction(&shipment_id), Some(us.clone()));
    let us_shipments = client.get_shipments_by_jurisdiction(&us);
    assert_eq!(us_shipments.len(), 1);
    assert!(us_shipments.contains(shipment_id));
}

#[test]
fn test_duplicate_shipment_id_does_not_retag_or_reindex() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    let us = Symbol::new(&t.env, "US");
    let eu = Symbol::new(&t.env, "EU");
    let shipment_id = create_tagged_shipment(&t, &t.buyer, "JUR-DUP-1", Some(us.clone()));

    let mut eu_opts = default_options(&t.env);
    eu_opts.jurisdiction = Some(eu.clone());
    let result = client.try_create_shipment(
        &shipment_id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &build_milestones(&t.env),
        &eu_opts,
    );
    assert!(result.is_err());

    assert_eq!(client.get_shipment_jurisdiction(&shipment_id), Some(us.clone()));
    assert_eq!(client.get_shipments_by_jurisdiction(&us).len(), 1);
    assert_eq!(client.get_shipments_by_jurisdiction(&eu).len(), 0);
}
