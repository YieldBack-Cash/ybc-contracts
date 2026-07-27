#![cfg(test)]

use crate::{Treasury, TreasuryClient};
use mock_vault::{MockVault, MockVaultClient};
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env, String,
};

struct TreasuryTest {
    env: Env,
    owner: Address,
    treasury_addr: Address,
    treasury: TreasuryClient<'static>,
}

impl TreasuryTest {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let owner = Address::generate(&env);
        let treasury_addr = env.register(Treasury, (&owner,));
        let treasury = TreasuryClient::new(&env, &treasury_addr);

        TreasuryTest {
            env,
            owner,
            treasury_addr,
            treasury,
        }
    }

    /// Registers a mock vault share token (Soroban token contract) and mints
    /// `amount` of it to the treasury — the shape fees from the AMM reserve
    /// fee and YM surplus sweep arrive in.
    fn fund_with_vault_shares(&self, amount: i128) -> Address {
        let vault_addr = self.env.register(
            MockVault,
            (
                &self.owner,
                String::from_str(&self.env, "Mock Vault Token"),
                String::from_str(&self.env, "MVT"),
                7u32,
            ),
        );
        MockVaultClient::new(&self.env, &vault_addr).mint(&self.treasury_addr, &amount);
        vault_addr
    }

    /// Registers a Stellar Asset Contract and mints `amount` of it to the
    /// treasury — the shape a classic-asset fee token would take.
    fn fund_with_sac(&self, amount: i128) -> Address {
        let issuer = Address::generate(&self.env);
        let sac_addr = self
            .env
            .register_stellar_asset_contract_v2(issuer)
            .address();
        StellarAssetClient::new(&self.env, &sac_addr).mint(&self.treasury_addr, &amount);
        sac_addr
    }

    fn transfer_deadline(&self) -> u32 {
        self.env.ledger().sequence() + 1000
    }
}

#[test]
fn constructor_sets_owner() {
    let t = TreasuryTest::setup();
    assert_eq!(t.treasury.get_owner(), Some(t.owner.clone()));
}

#[test]
fn withdraw_vault_shares() {
    let t = TreasuryTest::setup();
    let token = t.fund_with_vault_shares(1_000_0000000);
    let recipient = Address::generate(&t.env);

    t.treasury.withdraw(&token, &recipient, &400_0000000);

    let token_client = TokenClient::new(&t.env, &token);
    assert_eq!(token_client.balance(&recipient), 400_0000000);
    assert_eq!(token_client.balance(&t.treasury_addr), 600_0000000);
}

#[test]
fn withdraw_sac() {
    let t = TreasuryTest::setup();
    let token = t.fund_with_sac(1_000_0000000);
    let recipient = Address::generate(&t.env);

    t.treasury.withdraw(&token, &recipient, &400_0000000);

    let token_client = TokenClient::new(&t.env, &token);
    assert_eq!(token_client.balance(&recipient), 400_0000000);
    assert_eq!(token_client.balance(&t.treasury_addr), 600_0000000);
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn withdraw_rejects_zero_amount() {
    let t = TreasuryTest::setup();
    let token = t.fund_with_vault_shares(1_000_0000000);
    t.treasury.withdraw(&token, &t.owner, &0);
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn withdraw_rejects_negative_amount() {
    let t = TreasuryTest::setup();
    let token = t.fund_with_vault_shares(1_000_0000000);
    t.treasury.withdraw(&token, &t.owner, &-1);
}

#[test]
fn withdraw_more_than_balance_fails() {
    let t = TreasuryTest::setup();
    let vault_token = t.fund_with_vault_shares(100);
    let sac_token = t.fund_with_sac(100);
    let recipient = Address::generate(&t.env);

    assert!(t.treasury.try_withdraw(&vault_token, &recipient, &101).is_err());
    assert!(t.treasury.try_withdraw(&sac_token, &recipient, &101).is_err());
}

#[test]
fn withdraw_requires_owner_auth() {
    let t = TreasuryTest::setup();
    let token = t.fund_with_vault_shares(1_000_0000000);
    let recipient = Address::generate(&t.env);

    // Drop the blanket auth mock: with no signatures at all, only_owner's
    // enforce_owner_auth must reject the call.
    t.env.set_auths(&[]);
    assert!(t.treasury.try_withdraw(&token, &recipient, &1).is_err());
}

#[test]
fn ownership_transfer_is_two_step() {
    let t = TreasuryTest::setup();
    let new_owner = Address::generate(&t.env);

    t.treasury.transfer_ownership(&new_owner, &t.transfer_deadline());
    // Proposal alone moves nothing.
    assert_eq!(t.treasury.get_owner(), Some(t.owner.clone()));

    t.treasury.accept_ownership();
    assert_eq!(t.treasury.get_owner(), Some(new_owner));
}

#[test]
fn pending_transfer_can_be_cancelled() {
    let t = TreasuryTest::setup();
    let new_owner = Address::generate(&t.env);

    t.treasury.transfer_ownership(&new_owner, &t.transfer_deadline());
    // live_until_ledger = 0 cancels the pending transfer…
    t.treasury.transfer_ownership(&new_owner, &0);

    // …after which there is nothing to accept.
    assert!(t.treasury.try_accept_ownership().is_err());
    assert_eq!(t.treasury.get_owner(), Some(t.owner.clone()));
}

#[test]
fn accept_without_pending_fails() {
    let t = TreasuryTest::setup();
    assert!(t.treasury.try_accept_ownership().is_err());
}

/// Renouncing is possible but self-defeating for a treasury: with no owner,
/// every only_owner function — including withdraw — is permanently bricked.
#[test]
fn renounce_bricks_withdraw() {
    let t = TreasuryTest::setup();
    let token = t.fund_with_vault_shares(100);

    t.treasury.renounce_ownership();
    assert_eq!(t.treasury.get_owner(), None);
    assert!(t.treasury.try_withdraw(&token, &t.owner, &1).is_err());
}