//! Proof-of-concept tests for the YT yield-accrual solvency bug.
//!
//! `YieldToken::accrue_yield` computes pending yield as
//! `balance * (current_rate - old_index) / old_index`
//! (contracts/tokens/yield_token/src/contract.rs), which is an amount in
//! *asset* units. `claim_yield` then hands that number to the yield manager's
//! `distribute_yield`, which pays it out in vault *shares*. Because one share
//! is worth `rate / SCALAR_7` assets, the payout is inflated by the exchange
//! rate whenever the vault rate exceeds 1.0 — the YM pays out principal backing
//! as if it were yield.
//!
//! Both tests below assert the CORRECT (solvent) behaviour, so they fail on the
//! current code and will pass once the accrual formula is fixed to
//! `balance * (current_rate - old_index) * SCALAR_7 / (old_index * current_rate)`.

use soroban_sdk::{testutils::Address as _, Address, Env};

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};

const SCALAR_7: i128 = 10_000_000;

/// Single depositor: doubling the vault rate must not let the claimer withdraw
/// more shares than the yield actually earned, and must leave the YM able to
/// still redeem every outstanding PT.
#[test]
fn claim_yield_stays_within_earned_yield_and_keeps_ym_solvent() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    // Rate starts at 1.0. Deposit 100 shares → 100 PT + 100 YT. The YM now
    // holds those 100 shares as the sole backing for both the principal (PT)
    // and the yield (YT).
    let deposit = 100_000_000i128;
    f.ym_deposit(&f.user, deposit);
    assert_eq!(f.vault.balance(&f.yield_manager), deposit);
    assert_eq!(f.pt_balance(&f.user), deposit);
    assert_eq!(f.yt_balance(&f.user), deposit);

    // Vault rate doubles: the deposited shares are now worth 200 assets — 100
    // principal + 100 yield. 100 assets of yield is 50 shares at the new 2.0
    // rate, so a correct claim pays at most 50 shares.
    f.vault.set_exchange_rate(&(2 * SCALAR_7));
    let earned_yield_shares = deposit / 2; // 50_000_000

    let claimed = f.yt_claim_yield(&f.user);

    assert!(
        claimed <= earned_yield_shares,
        "claim_yield paid {} vault shares but the position only earned {} shares \
         of yield — the extra {} was drawn from principal backing",
        claimed,
        earned_yield_shares,
        claimed - earned_yield_shares,
    );

    // Independent solvency check: after the claim the YM must still hold enough
    // shares to redeem all outstanding PT at the current rate (100 PT / 2.0).
    let rate = f.ym_exchange_rate();
    let principal_shares_owed = f.pt_balance(&f.user) * SCALAR_7 / rate;
    assert!(
        f.vault.balance(&f.yield_manager) >= principal_shares_owed,
        "YM insolvent: holds {} shares but owes {} to back {} outstanding PT",
        f.vault.balance(&f.yield_manager),
        principal_shares_owed,
        f.pt_balance(&f.user),
    );
}

/// Two depositors sharing one YM: one user's over-claim must not be payable out
/// of the other user's principal. Alice claims + redeems after a rate doubling;
/// the YM must still be able to honour Bob's untouched position.
#[test]
fn over_claim_by_one_user_cannot_drain_another_users_backing() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let alice = f.user.clone();
    let bob = Address::generate(&env);
    f.vault.mint(&bob, &1_000_000_000);

    // Both deposit 100 shares at rate 1.0; the YM now backs both, holding 200.
    let deposit = 100_000_000i128;
    f.ym_deposit(&alice, deposit);
    f.ym_deposit_to(&f.vault.address, &f.yield_manager, &bob, deposit);
    assert_eq!(f.vault.balance(&f.yield_manager), 2 * deposit);

    // Rate doubles before maturity, then the market matures (locking the rate
    // at 2.0) so Alice can redeem her principal.
    f.vault.set_exchange_rate(&(2 * SCALAR_7));
    f.advance_time(ONE_YEAR_SECS + 1);

    // Alice takes everything she can: yield claim, then principal redemption.
    f.yt_claim_yield(&alice);
    f.ym_redeem_principal(&alice, f.pt_balance(&alice));

    // Bob's position is untouched: 100 PT + 100 YT. Redeeming both together
    // reconstitutes his original deposit, so his fair entitlement is the full
    // 100 shares he put in. The YM must still hold at least that much to
    // honour him — anything less means Alice's claim came out of Bob's money.
    let _ = f.ym_exchange_rate();
    assert!(
        f.vault.balance(&f.yield_manager) >= deposit,
        "Alice's over-claim drained the YM to {} shares, below Bob's untouched \
         {}-share entitlement — one user's yield claim stole another's backing",
        f.vault.balance(&f.yield_manager),
        deposit,
    );
}