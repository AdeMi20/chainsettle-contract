#![cfg(test)]

//! #563 – Fund escrow via token allowance (transfer_from).

extern crate std;

use super::*;
use crate::test_common::{default_options, setup, single_buyer_vec, build_milestones};
use soroban_sdk::{testutils::Address as _, token, Address, String};

fn sid(env: &Env, id: &str) -> String {
    String::from_str(env, id)
}

fn allowance_params(
    t: &crate::test_common::TestSetup,
    spender: &Address,
    from: &Address,
    shipment_id: &str,
    amount: i128,
) -> AllowanceShipmentParams {
    AllowanceShipmentParams {
        spender: spender.clone(),
        from: from.clone(),
        shipment_id: sid(&t.env, shipment_id),
        buyers: single_buyer_vec(&t.env, from),
        supplier: t.supplier.clone(),
        logistics: t.logistics.clone(),
        arbiter: t.arbiter.clone(),
        token: t.token_id.clone(),
        total_amount: amount,
        milestones: build_milestones(&t.env),
        options: default_options(&t.env),
    }
}

#[test]
fn funding_works_with_sufficient_allowance() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let spender = Address::generate(&t.env);
    let amount = 1_000_000i128;

    let sac = token::Client::new(&t.env, &t.token_id);
    // SAC approve flow: buyer grants spender allowance.
    sac.approve(&t.buyer, &spender, &amount, &1000);

    let params = allowance_params(&t, &spender, &t.buyer, "ALLOW-OK", amount);
    let id = client.create_shipment_with_allowance(&params);
    let shipment = client.get_shipment(&id);
    assert_eq!(shipment.total_amount, amount);
    assert_eq!(shipment.buyers.get(0).unwrap(), t.buyer);
    assert_eq!(shipment.status, ShipmentStatus::Active);
}

#[test]
#[should_panic]
fn fails_with_insufficient_allowance() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let spender = Address::generate(&t.env);
    let amount = 1_000_000i128;

    let sac = token::Client::new(&t.env, &t.token_id);
    sac.approve(&t.buyer, &spender, &100i128, &1000); // too small

    let params = allowance_params(&t, &spender, &t.buyer, "ALLOW-LOW", amount);
    client.create_shipment_with_allowance(&params);
}

#[test]
fn resulting_shipment_identical_to_normal() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let spender = Address::generate(&t.env);
    let amount = 1_000_000i128;

    // Normal shipment.
    client.create_shipment(
        &sid(&t.env, "NORMAL"),
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &amount,
        &build_milestones(&t.env),
        &default_options(&t.env),
    );
    let normal = client.get_shipment(&sid(&t.env, "NORMAL"));

    // Allowance-funded shipment.
    let sac = token::Client::new(&t.env, &t.token_id);
    sac.approve(&t.buyer2, &spender, &amount, &1000);
    // Mint already done for buyer2 in setup.
    let params = allowance_params(&t, &spender, &t.buyer2, "ALLOW-ID", amount);
    client.create_shipment_with_allowance(&params);
    let via_allowance = client.get_shipment(&sid(&t.env, "ALLOW-ID"));

    assert_eq!(normal.total_amount, via_allowance.total_amount);
    assert_eq!(normal.status, via_allowance.status);
    assert_eq!(normal.milestones.len(), via_allowance.milestones.len());
    assert_eq!(normal.supplier, via_allowance.supplier);
}

#[test]
#[should_panic(expected = "from must be one of the buyers")]
fn from_must_be_a_buyer() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let spender = Address::generate(&t.env);
    let stranger = Address::generate(&t.env);

    let sac = token::Client::new(&t.env, &t.token_id);
    token::StellarAssetClient::new(&t.env, &t.token_id).mint(&stranger, &5_000_000);
    sac.approve(&stranger, &spender, &1_000_000i128, &1000);

    let mut params = allowance_params(&t, &spender, &t.buyer, "ALLOW-BAD", 1_000_000);
    params.from = stranger;
    client.create_shipment_with_allowance(&params);
}
