//! Pins the exact authorization entries a wallet must produce for the YT swap flows.
//!
//! Unlike the rest of the suite (which runs under `mock_all_auths`), these tests use
//! `mock_auths` with explicit entry trees. `mock_auths` enforces the real matching
//! rules: `require_auth` only passes if a provided entry matches the invocation's
//! contract, function, and exact argument values. This is what production signing
//! looks like (minus the cryptography), so these tests guarantee two properties:
//!
//! 1. Every value in the user's signed tree is chosen client-side (`yt_out`, `max_v_in`,
//!    `yt_in`) — nothing computed on-chain appears, so a signature can never be
//!    invalidated by pool trades or vault-rate ticks between simulation and inclusion.
//! 2. The args really are enforced — a tree signed for a different amount is rejected.

use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
use soroban_sdk::{Env, IntoVal};

use super::fixture::IntegrationFixture;

const POOL_PT: i128 = 50_000_000;
const POOL_V: i128 = 50_000_000;
const YM_DEPOSIT: i128 = 100_000_000;

/// Same seeding as router_swaps: vault rate 1.0, user holds PT+YT, pool funded.
/// Setup runs under mock_all_auths; each test then switches to explicit entries.
fn seeded<'a>(env: &'a Env) -> IntegrationFixture<'a> {
    let f = IntegrationFixture::new(env);
    f.vault.set_exchange_rate(&10_000_000);
    f.ym_deposit(&f.user, YM_DEPOSIT);
    f.amm_deposit(&f.user, POOL_PT, POOL_V);
    f
}

// ── buy YT (swap_v_for_yt) ────────────────────────────────────────────────────

#[test]
fn test_buy_yt_auth_entry_contains_only_user_chosen_values() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_out = 1_000_000i128;
    let max_v_in = 1_000_000i128;
    let yt_before = f.yt_balance(&f.user);

    // The full tree the user signs: the router call, plus one vault transfer whose
    // amount is max_v_in. Both are values the user picked before signing — the
    // on-chain-computed user_cost appears nowhere.
    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router,
            fn_name: "swap_v_for_yt",
            args: (&f.vault.address, f.maturity, &f.user, yt_out, max_v_in).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.vault.address,
                fn_name: "transfer",
                args: (&f.user, &f.yield_manager, max_v_in).into_val(&env),
                sub_invokes: &[],
            }],
        },
    }]);

    f.router_swap_v_for_yt(&f.user, yt_out, max_v_in);

    assert_eq!(f.yt_balance(&f.user), yt_before + yt_out, "buy succeeds with only user-chosen signed values");
}

#[test]
#[should_panic]
fn test_buy_yt_auth_entry_wrong_amount_rejected() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_out = 1_000_000i128;
    let max_v_in = 1_000_000i128;

    // Identical tree but the transfer is signed for max_v_in - 1. The contract pulls
    // exactly max_v_in, so no entry matches and the swap must revert — proving the
    // matching is enforced (and that these tests aren't silently running mock-all).
    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router,
            fn_name: "swap_v_for_yt",
            args: (&f.vault.address, f.maturity, &f.user, yt_out, max_v_in).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.vault.address,
                fn_name: "transfer",
                args: (&f.user, &f.yield_manager, max_v_in - 1).into_val(&env),
                sub_invokes: &[],
            }],
        },
    }]);

    f.router_swap_v_for_yt(&f.user, yt_out, max_v_in);
}

// ── sell YT (swap_yt_for_v) ───────────────────────────────────────────────────

#[test]
fn test_sell_yt_auth_entry_contains_only_user_chosen_values() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_in = 1_000_000i128;
    let min_v_out = 1i128;
    let yt_before = f.yt_balance(&f.user);

    // The full tree the user signs: the router call plus a plain YT transfer of yt_in.
    // The exchange rate is no longer anywhere in the signed args — it travels in the
    // YM's own invocations, so a vault-rate tick can't invalidate this signature.
    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router,
            fn_name: "swap_yt_for_v",
            args: (&f.vault.address, f.maturity, &f.user, yt_in, min_v_out).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.yt,
                fn_name: "transfer",
                args: (&f.user, &f.yield_manager, yt_in).into_val(&env),
                sub_invokes: &[],
            }],
        },
    }]);

    f.router_swap_yt_for_v(&f.user, yt_in, min_v_out);

    assert_eq!(f.yt_balance(&f.user), yt_before - yt_in, "sell succeeds with only user-chosen signed values");
}

#[test]
#[should_panic]
fn test_sell_yt_auth_entry_wrong_amount_rejected() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_in = 1_000_000i128;
    let min_v_out = 1i128;

    // Signed for one less YT than the router pulls → no matching entry → revert.
    env.mock_auths(&[MockAuth {
        address: &f.user,
        invoke: &MockAuthInvoke {
            contract: &f.router,
            fn_name: "swap_yt_for_v",
            args: (&f.vault.address, f.maturity, &f.user, yt_in, min_v_out).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &f.yt,
                fn_name: "transfer",
                args: (&f.user, &f.yield_manager, yt_in - 1).into_val(&env),
                sub_invokes: &[],
            }],
        },
    }]);

    f.router_swap_yt_for_v(&f.user, yt_in, min_v_out);
}