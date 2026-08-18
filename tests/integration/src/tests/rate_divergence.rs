//! The pool must price PT against the yield manager's rate, not the vault's.
//!
//! PT is a claim on `face / rate` vault shares where `rate` is the YM's — that
//! is what `redeem_principal` settles at. The vault's own rate never enters PT's
//! payout, so pricing PT against it is wrong whenever the two differ.
//!
//! They differ exactly when a vault loses value. The YM high-water-marks
//! (`update_exchange_rate` keeps the stored value when the vault reports lower);
//! a direct vault read follows it down. That divergence is by design and these
//! tests still assert it — what changed is which number the AMM follows.
//!
//! Before the fix the pool read `convert_to_assets` off the vault, so a
//! drawdown left it valuing PT face at the depressed rate while the YM would
//! only ever redeem at the high-water mark. The pool overpaid for PT, and
//! anyone could mint PT at the YM's rate and sell it at the pool's:
//!
//! ```text
//!   7.3% vault loss:  +760bp on stake, plus the whole YT leg free
//!   1%   vault loss:  +78bp,           plus the whole YT leg free
//! ```
//!
//! Now the pool reads the YM, and a vault loss changes nothing about what a
//! round trip pays — `a_vault_loss_no_longer_changes_the_round_trip` pins that
//! as an exact equality against the healthy-market case.

use soroban_sdk::{testutils::Address as _, Address, Env};

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};

/// 1.0 in the 1e7 fixed-point the vault rate uses.
const RATE_ONE: i128 = 10_000_000;
/// The high-water mark these tests establish before the loss: 1.10.
const HWM: i128 = 11_000_000;
/// The rate after the loss: 1.02, a ~7.3% drawdown.
const LIVE_AFTER_LOSS: i128 = 10_200_000;
/// A 1% drawdown — the smallest loss that used to pay.
const LIVE_SMALL_LOSS: i128 = HWM * 99 / 100;

const POOL_PT: i128 = 400_000_000;
const POOL_V: i128 = 400_000_000;
const ACTOR_V: i128 = 200_000_000;
const STAKE: i128 = 50_000_000;

/// Seeds a deep pool, walks the vault up to `hwm` over most of the market's
/// life, then sets the live rate to `live`. Returns a funded fresh actor.
///
/// Passing `live == hwm` gives the same market with no drawdown, which is what
/// the equality assertions compare against.
fn diverged_market(f: &IntegrationFixture, hwm: i128, live: i128) -> Address {
    // The LP seeds the pool the ordinary way: split shares, then deposit both legs.
    f.ym_deposit(&f.user, 500_000_000);
    f.amm_deposit(&f.user, POOL_PT, POOL_V);

    // A year of ordinary life, less a week. The remaining time to expiry sets the
    // PT discount, and a short one is the interesting case: it pushes PT close to
    // face, which is when a small rate drop used to bite hardest.
    f.advance_time(ONE_YEAR_SECS - 7 * 24 * 3600);

    f.vault.set_exchange_rate(&hwm);
    // Touch the YM so the high-water mark actually records `hwm`.
    assert_eq!(f.ym_exchange_rate(), hwm, "YM should have taken the new high");

    // The vault takes a loss.
    f.vault.set_exchange_rate(&live);
    assert_eq!(
        f.ym_exchange_rate(),
        hwm,
        "YM must hold the high-water mark through a drawdown"
    );
    assert_eq!(
        f.vault.convert_to_assets(&RATE_ONE),
        live,
        "vault must report the depressed rate"
    );

    let actor = Address::generate(&f.env);
    f.vault.mint(&actor, &ACTOR_V);
    actor
}

/// Mint PT+YT from the YM, immediately sell the PT to the pool, and report the
/// net change in vault shares. This is the arbitrage the divergence used to pay
/// for: buy face at the YM's rate, sell it at the pool's.
fn mint_and_dump_profit(hwm: i128, live: i128) -> i128 {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    let actor = diverged_market(&f, hwm, live);

    let before = f.vault.balance(&actor);
    f.ym_deposit(&actor, STAKE);
    let pt = f.pt_balance(&actor);
    assert_eq!(pt, STAKE * hwm / RATE_ONE, "minted at the YM's rate");
    f.pool.swap_pt_for_v(&actor, &pt, &1);

    f.vault.balance(&actor) - before
}

/// The YM and the vault still diverge after a loss — that part is deliberate and
/// unchanged. The high-water mark is what makes PT a fixed claim.
#[test]
fn the_ym_still_high_water_marks_against_the_vault() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    diverged_market(&f, HWM, LIVE_AFTER_LOSS);

    assert_eq!(f.ym_exchange_rate(), HWM);
    assert_eq!(f.vault.convert_to_assets(&RATE_ONE), LIVE_AFTER_LOSS);
}

/// The regression test.
///
/// A vault drawdown must not change what a mint-and-dump round trip pays. Exact
/// equality against the healthy market is the strongest form of that: if the
/// pool ever reads the vault's rate again, these numbers separate immediately
/// and by roughly the size of the drawdown.
#[test]
fn a_vault_loss_no_longer_changes_the_round_trip() {
    let healthy = mint_and_dump_profit(HWM, HWM);
    let after_loss = mint_and_dump_profit(HWM, LIVE_AFTER_LOSS);
    let after_small_loss = mint_and_dump_profit(HWM, LIVE_SMALL_LOSS);

    std::println!(
        "round trip — healthy {healthy}, 7.3% loss {after_loss}, 1% loss {after_small_loss}"
    );

    assert!(
        healthy < 0,
        "the round trip must cost the spread even in a healthy market, got {healthy}"
    );
    assert_eq!(
        after_loss, healthy,
        "a 7.3% vault loss changed the round trip: {after_loss} vs {healthy}"
    );
    assert_eq!(
        after_small_loss, healthy,
        "a 1% vault loss changed the round trip: {after_small_loss} vs {healthy}"
    );
}

/// Stated on its own, because it is the property that was violated: minting face
/// from the YM and selling it to the pool must never pay. It used to pay 760bp
/// on a 7.3% drawdown and 78bp on a 1% one.
#[test]
fn mint_and_dump_never_pays() {
    for (label, live) in [
        ("healthy", HWM),
        ("1% loss", LIVE_SMALL_LOSS),
        ("7.3% loss", LIVE_AFTER_LOSS),
        ("30% loss", HWM * 70 / 100),
    ] {
        let profit = mint_and_dump_profit(HWM, live);
        std::println!("mint-and-dump under {label}: {profit}");
        assert!(
            profit < 0,
            "mint-and-dump paid {profit} under {label} — the pool is mispricing PT"
        );
    }
}

/// Buying YT used to revert through a drawdown: the pool advanced V priced at
/// the vault's rate while the YM sized the mint at the high-water mark, so
/// `user_cost` went negative and `assert!(user_cost > 0)` fired. With one rate
/// there is nothing to diverge and the zap works.
#[test]
fn buying_yt_works_through_a_vault_loss() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    let actor = diverged_market(&f, HWM, LIVE_AFTER_LOSS);

    f.router_swap_v_for_yt(&actor, 50_000_000, 20_000_000);
    assert!(f.yt_balance(&actor) >= 50_000_000, "bought YT");
}

/// The mirror image: selling YT used to fail `assert!(shares_returned >= v_owed)`
/// on the same condition that made mint-and-dump profitable.
#[test]
fn selling_yt_works_through_a_vault_loss() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    let actor = diverged_market(&f, HWM, LIVE_AFTER_LOSS);

    f.ym_deposit(&actor, STAKE);
    let yt = f.yt_balance(&actor);
    let v_before = f.vault.balance(&actor);

    f.router_swap_yt_for_v(&actor, yt, 1);

    assert_eq!(f.yt_balance(&actor), 0, "YT sold");
    assert!(f.vault.balance(&actor) > v_before, "received V for the YT");
}

/// Both zaps on a market that never took a loss, so the two tests above are
/// evidence about the drawdown rather than about the trade size.
#[test]
fn both_zaps_work_when_the_vault_never_lost_value() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    let actor = diverged_market(&f, HWM, HWM);

    f.router_swap_v_for_yt(&actor, 50_000_000, 20_000_000);
    assert!(f.yt_balance(&actor) >= 50_000_000, "bought YT");

    f.router_swap_yt_for_v(&actor, 50_000_000, 1);
}