#![cfg(test)]

//! #561 – Buyer escrow vault for pre-funded shipments.

extern crate std;

use super::*;
use crate::test_common::{default_options, setup, single_buyer_vec, build_milestones};
use soroban_sdk::{testutils::Address as _, token, Address, String};

fn sid(env: &Env, id: &str) -> String {
    String::from_str(env, id)
}

#[test]
fn vault_deposit_withdraw_and_balance() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    client.vault_deposit(&t.buyer, &t.token_id, &5_000_000i128);
    assert_eq!(client.get_vault_balance(&t.buyer, &t.token_id), 5_000_000);

    client.vault_withdraw(&t.buyer, &t.token_id, &2_000_000i128);
    assert_eq!(client.get_vault_balance(&t.buyer, &t.token_id), 3_000_000);
}

#[test]
#[should_panic(expected = "insufficient vault balance")]
fn withdraw_cannot_exceed_balance() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.vault_deposit(&t.buyer, &t.token_id, &100i128);
    client.vault_withdraw(&t.buyer, &t.token_id, &101i128);
}

#[test]
#[should_panic(expected = "insufficient vault balance")]
fn create_with_insufficient_vault_fails() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.vault_deposit(&t.buyer, &t.token_id, &100i128);

    let mut opts = default_options(&t.env);
    opts.fund_from_vault = true;
    client.create_shipment(
        &sid(&t.env, "VAULT-LOW"),
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &1_000_000i128,
        &build_milestones(&t.env),
        &opts,
    );
}

#[test]
fn create_from_vault_debits_and_refunds_to_vault() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    let amount = 1_000_000i128;
    client.vault_deposit(&t.buyer, &t.token_id, &amount);

    let mut opts = default_options(&t.env);
    opts.fund_from_vault = true;
    let id = sid(&t.env, "VAULT-OK");
    client.create_shipment(
        &id,
        &single_buyer_vec(&t.env, &t.buyer),
        &t.supplier,
        &t.logistics,
        &t.arbiter,
        &t.token_id,
        &amount,
        &build_milestones(&t.env),
        &opts,
    );
    assert_eq!(client.get_vault_balance(&t.buyer, &t.token_id), 0);

    client.cancel_shipment(&t.buyer, &id);
    assert_eq!(client.get_vault_balance(&t.buyer, &t.token_id), amount);
}

#[test]
fn withdraw_treasury_dust_excludes_vault() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.vault_deposit(&t.buyer, &t.token_id, &1_000_000i128);

    // No escrow, only vault — dust withdrawal must fail (vault is reserved).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.withdraw_treasury_dust(&t.buyer, &t.token_id, &t.treasury);
    }));
    assert!(result.is_err(), "dust withdraw must not touch vault balances");
}

#[test]
#[should_panic(expected = "amount must be greater than zero")]
fn vault_deposit_rejects_zero() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);
    client.vault_deposit(&t.buyer, &t.token_id, &0i128);
}

#[test]
fn vault_balances_tracked_per_buyer_and_token() {
    let t = setup();
    let client = ChainSettleContractClient::new(&t.env, &t.contract_id);

    // Second token.
    let token_admin = Address::generate(&t.env);
    let token2 = t
        .env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    token::StellarAssetClient::new(&t.env, &token2).mint(&t.buyer, &1_000_000);
    token::StellarAssetClient::new(&t.env, &token2).mint(&t.buyer2, &1_000_000);

    client.vault_deposit(&t.buyer, &t.token_id, &100i128);
    client.vault_deposit(&t.buyer, &token2, &200i128);
    client.vault_deposit(&t.buyer2, &t.token_id, &300i128);

    assert_eq!(client.get_vault_balance(&t.buyer, &t.token_id), 100);
    assert_eq!(client.get_vault_balance(&t.buyer, &token2), 200);
    assert_eq!(client.get_vault_balance(&t.buyer2, &t.token_id), 300);
    assert_eq!(client.get_vault_balance(&t.buyer2, &token2), 0);
}
