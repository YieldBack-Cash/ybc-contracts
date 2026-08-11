//! Pins the exact authorization a wallet must produce for a zap.
//!
//! Every other zap test runs under `mock_all_auths`, which accepts any tree —
//! so the whole suite passed while the first on-chain zap failed instantly with
//! Auth/InvalidAction. These use `mock_auths`, which enforces the real matching
//! rules: an entry satisfies `require_auth` only if the contract, function and
//! **exact argument values** all match.
//!
//! The property being guarded is narrow and load-bearing: *a user's signed tree
//! contains only values the user chose*. A wallet builds that tree by simulating
//! first and the transaction executes a ledger or more later, so any argument
//! the chain computes in between — a vault share count, a pool-priced amount,
//! `env.ledger().sequence()` — is a mismatch waiting to happen. Against a vault
//! whose rate accrues every ledger, it is a guaranteed one.
//!
//! What broke on testnet, concretely: `zap_asset_for_split` had the router call
//! `approve(user, ym, <measured shares>, <current ledger>)`. Both arguments were
//! execution-time values. It now forwards to the YM's asset entrypoint instead,
//! which deposits with itself as receiver — no allowance, nothing measured.

use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
use soroban_sdk::{Env, IntoVal};

use super::zap_fixture::ZapFixture;

/// The split path signs the least of any zap: the router call, the YM call
/// beneath it, and the vault deposit that moves the user's asset. Every value
/// in the tree is one the caller picked.
#[test]
fn split_zap_signs_only_caller_chosen_values() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let asset_in = 100_000_000i128;
    let min_tokens_out = 1i128;
    let pt_before = f.balance(&f.pt);

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "zap_asset_for_split",
            args: (&f.vault, f.maturity, &f.user, asset_in, min_tokens_out).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.ym,
                fn_name: "deposit_asset",
                args: (&f.user, asset_in, min_tokens_out).into_val(&env),
                sub_invokes: &[MockAuthInvoke {
                    // Vault deposit: `asset_in` is the caller's figure and the
                    // YM is the receiver, so the share count this mints appears
                    // nowhere in what the user signed.
                    contract: &f.vault,
                    fn_name: "deposit",
                    args: (asset_in, &f.ym, &f.user, &f.user).into_val(&env),
                    sub_invokes: &[MockAuthInvoke {
                        // The vault pulls the underlying itself, one frame
                        // deeper — also the caller's figure.
                        contract: &f.asset,
                        fn_name: "transfer",
                        args: (&f.user, &f.vault, asset_in).into_val(&env),
                        sub_invokes: &[],
                    }],
                }],
            }],
        },
    }]);

    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &asset_in, &min_tokens_out);

    assert!(minted > 0);
    assert_eq!(f.balance(&f.pt) - pt_before, minted);
}

/// A tree signed for a different amount must be rejected — otherwise the test
/// above would prove nothing about argument matching.
#[test]
#[should_panic]
fn split_zap_rejects_a_tree_signed_for_another_amount() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let signed_for = 100_000_000i128;
    let actually_called_with = 100_000_001i128;

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "zap_asset_for_split",
            args: (&f.vault, f.maturity, &f.user, signed_for, 1i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    f.router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &actually_called_with, &1);
}

/// The exit path's counterpart: burns are caller-chosen amounts, and the shares
/// owed are redeemed from the YM's own custody, so no measured figure appears.
#[test]
fn split_exit_signs_only_caller_chosen_values() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &100_000_000, &1);
    let asset_before = f.balance(&f.asset);

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "zap_split_for_asset",
            args: (&f.vault, f.maturity, &f.user, minted, 1i128).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.ym,
                fn_name: "redeem_combined_to_asset",
                args: (&f.user, minted, 1i128).into_val(&env),
                sub_invokes: &[MockAuthInvoke {
                    // The PT burn is the ONLY sub-entry: both arguments are the
                    // caller's. The YT burn is deliberately absent — it takes a
                    // live `exchange_rate`, so if it demanded the holder's
                    // signature the wallet would sign one rate and the chain
                    // would execute with another, one ledger later. It is
                    // admin-gated instead, with the YM authenticating the
                    // holder. Re-add `from.require_auth()` to `burn_with_rate`
                    // and this test fails — which is the point, because on-chain
                    // it failed as Auth/InvalidAction on every redeem.
                    contract: &f.pt,
                    fn_name: "burn",
                    args: (&f.user, minted).into_val(&env),
                    sub_invokes: &[],
                }],
            }],
        },
    }]);

    let returned = f
        .router
        .zap_split_for_asset(&f.vault, &f.maturity, &f.user, &minted, &1);

    assert!(returned > 0);
    assert_eq!(f.balance(&f.asset) - asset_before, returned);
}

/// The sweep allowance is the mechanism that lets a *measured* leftover be
/// converted without the measurement entering a signature: the user signs a
/// ceiling and an expiry they chose, and the router redeems as operator. Both
/// arguments here are fixed, which is the whole point.
#[test]
fn sweep_allowance_is_signed_with_caller_chosen_arguments() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let pt_out = 50_000_000i128;
    let max_asset_in = 200_000_000i128;
    let max_v_in = 100_000_000i128;
    let sweep = 10_000_000_000i128;
    let expiry = f.expiry();

    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router.address,
            fn_name: "zap_asset_for_pt",
            args: (
                &f.vault, f.maturity, &f.user, pt_out, max_asset_in, max_v_in, sweep, expiry,
            )
                .into_val(&env),
            sub_invokes: &[
                MockAuthInvoke {
                    contract: &f.vault,
                    fn_name: "deposit",
                    args: (max_asset_in, &f.user, &f.user, &f.user).into_val(&env),
                    sub_invokes: &[MockAuthInvoke {
                        contract: &f.asset,
                        fn_name: "transfer",
                        args: (&f.user, &f.vault, max_asset_in).into_val(&env),
                        sub_invokes: &[],
                    }],
                },
                MockAuthInvoke {
                    // The pool pulls the caller's bound, not the priced amount.
                    contract: &f.pool.address,
                    fn_name: "swap_v_for_pt",
                    args: (&f.user, pt_out, max_v_in).into_val(&env),
                    sub_invokes: &[MockAuthInvoke {
                        contract: &f.vault,
                        fn_name: "transfer",
                        args: (&f.user, &f.pool.address, max_v_in).into_val(&env),
                        sub_invokes: &[],
                    }],
                },
                MockAuthInvoke {
                    // Ceiling and expiry, both chosen before signing. The
                    // measured leftover is covered by, but absent from, this.
                    contract: &f.vault,
                    fn_name: "approve",
                    args: (&f.user, &f.router.address, sweep, expiry).into_val(&env),
                    sub_invokes: &[],
                },
            ],
        },
    }]);

    let shares_before = f.balance(&f.vault);
    let spent = f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &pt_out,
        &max_asset_in,
        &max_v_in,
        &sweep,
        &expiry,
    );

    assert!(spent > 0);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
}
