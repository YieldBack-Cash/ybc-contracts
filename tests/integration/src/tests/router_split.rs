//! `Router::split` / `Router::recombine` — the share-denominated pair.
//!
//! These wrap `YM::deposit` and `YM::redeem_combined`, which remain public and
//! directly callable. Routing buys market resolution through the factory and an
//! entrypoint that outlives market recreation; it is not a chokepoint and never
//! becomes one.
//!
//! The auth-entry tests below carry the weight. `split` is the only routed
//! operation where the router grants an allowance on the caller's behalf, which
//! is what keeps the YM's address out of the frontend — and it is the same shape
//! that failed on testnet when an earlier `zap_asset_for_split` approved with a
//! measured amount and `env.ledger().sequence()`. Here both arguments are the
//! caller's own, which is the difference. See the header of `zap_auth_entries.rs`.

use soroban_sdk::testutils::{Ledger as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{Env, IntoVal};

use super::zap_fixture::ZapFixture;

/// Vault shares the user is holding in every test below, deposited up front.
const SEED_ASSETS: i128 = 500_000_000;

/// Gives `f.user` vault shares to split, and returns how many they hold.
fn fund_shares(f: &ZapFixture) -> i128 {
    standard_vault::StandardVaultClient::new(&f.env, &f.vault)
        .deposit(&SEED_ASSETS, &f.user, &f.user, &f.user);
    f.balance(&f.vault)
}

// ── split ────────────────────────────────────────────────────────────────────

#[test]
fn split_mints_pt_and_yt_in_equal_measure() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares = fund_shares(&f);

    let pt_before = f.balance(&f.pt);
    let yt_before = f.balance(&f.yt);
    let split_amount = shares / 2;

    f.router
        .split(&f.vault, &f.maturity, &f.user, &split_amount, &f.expiry());

    let pt_minted = f.balance(&f.pt) - pt_before;
    let yt_minted = f.balance(&f.yt) - yt_before;

    assert!(pt_minted > 0, "split minted nothing");
    assert_eq!(pt_minted, yt_minted, "PT and YT must mint in equal measure");
    assert_eq!(
        f.balance(&f.vault),
        shares - split_amount,
        "exactly the split amount left the caller"
    );
}

/// The router's allowance is sized to the split and consumed by it, so the YM is
/// left with no standing claim on the caller's shares.
#[test]
fn split_leaves_no_residual_allowance() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares = fund_shares(&f);

    f.router
        .split(&f.vault, &f.maturity, &f.user, &(shares / 2), &f.expiry());

    let remaining =
        soroban_sdk::token::TokenClient::new(&env, &f.vault).allowance(&f.user, &f.ym);
    assert_eq!(remaining, 0, "allowance outlived the deposit");
}

#[test]
#[should_panic(expected = "shares_amount must be positive")]
fn split_rejects_a_non_positive_amount() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    fund_shares(&f);

    f.router.split(&f.vault, &f.maturity, &f.user, &0, &f.expiry());
}

#[test]
#[should_panic(expected = "no market for vault and maturity")]
fn split_rejects_an_unknown_market() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    fund_shares(&f);

    // Same vault, a maturity the factory never recorded.
    f.router
        .split(&f.vault, &(f.maturity + 1), &f.user, &1_000_000, &f.expiry());
}

// ── recombine ────────────────────────────────────────────────────────────────

#[test]
fn recombine_returns_the_shares_split_in() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares = fund_shares(&f);

    let split_amount = shares / 2;
    let pt_before = f.balance(&f.pt);

    f.router
        .split(&f.vault, &f.maturity, &f.user, &split_amount, &f.expiry());
    let minted = f.balance(&f.pt) - pt_before;

    let shares_before = f.balance(&f.vault);
    f.router
        .recombine(&f.vault, &f.maturity, &f.user, &minted);

    let returned = f.balance(&f.vault) - shares_before;
    assert_eq!(f.balance(&f.pt), pt_before, "PT not fully burned");
    // The round trip divides by the rate and then multiplies back, so it may
    // shed a stroop; it must never create one.
    assert!(returned <= split_amount, "round trip created shares");
    assert!(
        split_amount - returned <= 1,
        "round trip lost more than rounding: in {} out {}",
        split_amount,
        returned
    );
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn recombine_rejects_a_non_positive_amount() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    f.router.recombine(&f.vault, &f.maturity, &f.user, &0);
}

/// Past maturity the pair must go through `exit_expired` — recombining would
/// burn YT for no extra shares.
#[test]
#[should_panic]
fn recombine_reverts_after_maturity() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares = fund_shares(&f);

    f.router
        .split(&f.vault, &f.maturity, &f.user, &(shares / 2), &f.expiry());
    let held = f.balance(&f.pt);

    env.ledger().with_mut(|l| l.timestamp = f.maturity + 1);
    f.router.recombine(&f.vault, &f.maturity, &f.user, &held);
}

// ── auth entries ─────────────────────────────────────────────────────────────

/// The whole point of routing `split`: the caller signs the router call, the
/// allowance the router grants on their behalf, and the YM deposit beneath it.
/// The YM's address appears in that tree without the caller having resolved it,
/// and every *value* in the tree is one they chose.
///
/// The `transfer_from` that moves the shares is deliberately absent — the YM is
/// its spender and its own direct invoker, so it needs no signature.
#[test]
fn split_signs_only_caller_chosen_values() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares = fund_shares(&f);

    let split_amount = shares / 2;
    let expiry = f.expiry();
    let pt_before = f.balance(&f.pt);

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "split",
            args: (&f.vault, f.maturity, &f.user, split_amount, expiry).into_val(&env),
            sub_invokes: &[
                MockAuthInvoke {
                    // Spender resolved by the router; amount and expiry chosen
                    // by the caller before signing.
                    contract: &f.vault,
                    fn_name: "approve",
                    args: (&f.user, &f.ym, split_amount, expiry).into_val(&env),
                    sub_invokes: &[],
                },
                MockAuthInvoke {
                    contract: &f.ym,
                    fn_name: "deposit",
                    args: (&f.user, split_amount).into_val(&env),
                    sub_invokes: &[],
                },
            ],
        },
    }]);

    f.router
        .split(&f.vault, &f.maturity, &f.user, &split_amount, &expiry);

    assert!(f.balance(&f.pt) > pt_before);
}

/// A tree signed for a different split size must be rejected — otherwise the
/// test above proves nothing about argument matching.
#[test]
#[should_panic]
fn split_rejects_a_tree_signed_for_another_amount() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    fund_shares(&f);

    let signed_for = 100_000_000i128;
    let actually_called_with = 100_000_001i128;
    let expiry = f.expiry();

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "split",
            args: (&f.vault, f.maturity, &f.user, signed_for, expiry).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    f.router
        .split(&f.vault, &f.maturity, &f.user, &actually_called_with, &expiry);
}

/// `recombine` signs less still: no allowance, and the PT burn is the only
/// sub-entry. The YT burn is absent because it takes a live `exchange_rate` and
/// is admin-gated rather than holder-gated — if it demanded the holder's
/// signature the wallet would sign one rate and the chain would execute with
/// another. Re-add `from.require_auth()` to `burn_with_rate` and this fails.
#[test]
fn recombine_signs_only_caller_chosen_values() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares = fund_shares(&f);

    let pt_before = f.balance(&f.pt);
    f.router
        .split(&f.vault, &f.maturity, &f.user, &(shares / 2), &f.expiry());
    let minted = f.balance(&f.pt) - pt_before;

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "recombine",
            args: (&f.vault, f.maturity, &f.user, minted).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.ym,
                fn_name: "redeem_combined",
                args: (&f.user, minted).into_val(&env),
                sub_invokes: &[MockAuthInvoke {
                    contract: &f.pt,
                    fn_name: "burn",
                    args: (&f.user, minted).into_val(&env),
                    sub_invokes: &[],
                }],
            }],
        },
    }]);

    f.router.recombine(&f.vault, &f.maturity, &f.user, &minted);

    assert_eq!(f.balance(&f.pt), pt_before, "PT not burned");
}
