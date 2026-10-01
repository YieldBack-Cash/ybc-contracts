//! The router's events, one per user-facing action.
//!
//! Each test performs one action and checks that the router published exactly
//! one event, named after the entrypoint, whose amounts are the user's actual
//! balance changes — so a reader can take a single record at face value. If a
//! zap ever started publishing its legs as separate events, or reporting a bound
//! where a measured amount belongs, one of these fails.
//!
//! `events().all()` holds only the most recent invocation, so every test reads
//! the event straight after the call and only then measures balances.

use soroban_sdk::{
    testutils::Events as _,
    xdr::{self, ScVal},
    Address, Env, Symbol, TryFromVal,
};

use super::zap_fixture::{ZapFixture, SWEEP};

/// The router's one event from the last invocation: its name, its two address
/// topics, and its data fields in order.
struct RouterEvent {
    name: Symbol,
    vault: Address,
    to: Address,
    data: std::vec::Vec<ScVal>,
}

fn last_router_event(f: &ZapFixture) -> RouterEvent {
    let events = f.env.events().all().filter_by_contract(&f.router.address);
    let events = events.events();
    assert_eq!(
        events.len(),
        1,
        "the router publishes exactly one event per action"
    );
    let xdr::ContractEventBody::V0(body) = events[0].body.clone();
    assert_eq!(body.topics.len(), 3, "name, vault, to");
    let ScVal::Vec(Some(data)) = body.data else {
        panic!("event data is not a vec");
    };
    RouterEvent {
        name: Symbol::try_from_val(&f.env, &body.topics[0]).expect("topic 0 is the name"),
        vault: Address::try_from_val(&f.env, &body.topics[1]).expect("topic 1 is the vault"),
        to: Address::try_from_val(&f.env, &body.topics[2]).expect("topic 2 is the user"),
        data: data.to_vec(),
    }
}

impl RouterEvent {
    fn i128(&self, i: usize) -> i128 {
        let ScVal::I128(parts) = &self.data[i] else {
            panic!("field {i} is not an i128");
        };
        ((parts.hi as i128) << 64) | parts.lo as i128
    }

    fn u64(&self, i: usize) -> u64 {
        let ScVal::U64(v) = &self.data[i] else {
            panic!("field {i} is not a u64");
        };
        *v
    }

    fn address(&self, e: &Env, i: usize) -> Address {
        Address::try_from_val(e, &self.data[i])
            .unwrap_or_else(|_| panic!("field {i} is not an address"))
    }

    /// Every router event starts the same way: named after the entrypoint,
    /// filed under the market and the user, `maturity` first in the data.
    fn assert_header(&self, f: &ZapFixture, name: &str) {
        assert_eq!(self.name, Symbol::new(&f.env, name), "event name");
        assert_eq!(self.vault, f.vault, "vault topic");
        assert_eq!(self.to, f.user, "user topic");
        assert_eq!(self.u64(0), f.maturity, "maturity field");
    }
}

#[test]
fn zap_asset_for_pt_reports_the_asset_actually_paid() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let asset_before = f.balance(&f.asset);

    let spent = f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_asset_for_pt");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(
        ev.i128(2),
        asset_before - f.balance(&f.asset),
        "asset_in is the net spend"
    );
    assert_eq!(ev.i128(2), spent);
    assert!(
        ev.i128(2) < 300_000_000,
        "the refunded part of the budget is not reported as spent"
    );
    assert_eq!(ev.i128(3), 100_000_000, "pt_out");
    assert_eq!(ev.data.len(), 4);
}

#[test]
fn zap_pt_for_asset_reports_the_asset_actually_received() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );
    let asset_before = f.balance(&f.asset);

    let received = f.router.zap_pt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_pt_for_asset");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(ev.i128(2), 100_000_000, "pt_in");
    assert_eq!(
        ev.i128(3),
        f.balance(&f.asset) - asset_before,
        "asset_out is what arrived"
    );
    assert_eq!(ev.i128(3), received);
    assert_eq!(ev.data.len(), 4);
}

#[test]
fn zap_asset_for_yt_and_back_report_asset_amounts_not_share_bounds() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let asset_before = f.balance(&f.asset);

    let spent = f.router.zap_asset_for_yt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_asset_for_yt");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(
        ev.i128(2),
        asset_before - f.balance(&f.asset),
        "asset_in is the net spend"
    );
    assert_eq!(ev.i128(2), spent);
    assert_ne!(
        ev.i128(2),
        200_000_000,
        "the share bound is not what was paid"
    );
    assert_eq!(ev.i128(3), 100_000_000, "yt_out");
    assert_eq!(ev.data.len(), 4);

    let asset_before = f.balance(&f.asset);
    let received = f.router.zap_yt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_yt_for_asset");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(ev.i128(2), 100_000_000, "yt_in");
    assert_eq!(
        ev.i128(3),
        f.balance(&f.asset) - asset_before,
        "asset_out is what arrived"
    );
    assert_eq!(ev.i128(3), received);
    assert_ne!(ev.i128(3), 1, "the floor is not what was received");
    assert_eq!(ev.data.len(), 4);
}

#[test]
fn zap_split_and_recombine_report_both_sides() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let pt_before = f.balance(&f.pt);

    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_asset_for_split");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(ev.i128(2), 500_000_000, "asset_in");
    assert_eq!(
        ev.i128(3),
        f.balance(&f.pt) - pt_before,
        "tokens_out is the PT (and YT) minted"
    );
    assert_eq!(ev.i128(3), minted);
    assert_eq!(ev.data.len(), 4);

    let asset_before = f.balance(&f.asset);
    let returned = f
        .router
        .zap_split_for_asset(&f.vault, &f.maturity, &f.user, &minted, &1);
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_split_for_asset");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(ev.i128(2), minted, "tokens_in");
    assert_eq!(
        ev.i128(3),
        f.balance(&f.asset) - asset_before,
        "asset_out is what arrived"
    );
    assert_eq!(ev.i128(3), returned);
    assert_eq!(ev.data.len(), 4);
}

#[test]
fn zap_lp_round_trip_reports_the_asset_paid_and_received() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let asset_before = f.balance(&f.asset);

    let lp_out = f.router.zap_asset_for_lp(
        &f.vault,
        &f.maturity,
        &f.user,
        &400_000_000,
        &200_000_000,
        &250_000_000,
        &150_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_asset_for_lp");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(
        ev.i128(2),
        asset_before - f.balance(&f.asset),
        "asset_in is the net spend"
    );
    assert!(
        ev.i128(2) < 400_000_000,
        "what the pool declined came back and is not reported as spent"
    );
    assert_eq!(ev.i128(3), 200_000_000, "pt_bought");
    assert_eq!(ev.i128(4), lp_out, "lp_out");
    assert_eq!(ev.data.len(), 5);

    let pt_held = f.balance(&f.pt);
    let asset_before = f.balance(&f.asset);
    let returned = f.router.zap_lp_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &lp_out,
        &pt_held,
        &1,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "zap_lp_for_asset");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(ev.i128(2), lp_out, "lp_in");
    assert_eq!(ev.i128(3), pt_held, "pt_sold");
    assert_eq!(
        ev.i128(4),
        f.balance(&f.asset) - asset_before,
        "asset_out is what arrived"
    );
    assert_eq!(ev.i128(4), returned);
    assert_eq!(ev.data.len(), 5);
}

#[test]
fn share_denominated_yt_swaps_report_the_shares_actually_moved() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let shares_before = f.balance(&f.vault);

    f.router
        .swap_v_for_yt(&f.vault, &f.maturity, &f.user, &100_000_000, &200_000_000);
    let ev = last_router_event(&f);

    ev.assert_header(&f, "swap_v_for_yt");
    assert_eq!(
        ev.i128(1),
        shares_before - f.balance(&f.vault),
        "v_in is the net share spend"
    );
    assert!(
        ev.i128(1) > 0 && ev.i128(1) < 200_000_000,
        "not the bound: {}",
        ev.i128(1)
    );
    assert_eq!(ev.i128(2), 100_000_000, "yt_out");
    assert_eq!(ev.data.len(), 3);

    let shares_before = f.balance(&f.vault);
    f.router
        .swap_yt_for_v(&f.vault, &f.maturity, &f.user, &100_000_000, &1);
    let ev = last_router_event(&f);

    ev.assert_header(&f, "swap_yt_for_v");
    assert_eq!(ev.i128(1), 100_000_000, "yt_in");
    assert_eq!(
        ev.i128(2),
        f.balance(&f.vault) - shares_before,
        "v_out is the net share gain"
    );
    assert!(ev.i128(2) > 1, "not the floor");
    assert_eq!(ev.data.len(), 3);
}

#[test]
fn exit_expired_to_asset_names_the_asset_and_the_amount() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let lp = f.pool.balance_shares(&f.user);
    f.accrue_yield(200_000_000);
    f.advance_past_maturity();
    let asset_before = f.balance(&f.asset);

    let returned = f.router.exit_expired_to_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &lp,
        &10_000_000_000,
        &f.expiry(),
        &1,
        &SWEEP,
        &f.expiry(),
    );
    let ev = last_router_event(&f);

    ev.assert_header(&f, "exit_expired_to_asset");
    assert_eq!(ev.address(&env, 1), f.asset);
    assert_eq!(ev.i128(2), lp, "lp_shares");
    assert_eq!(
        ev.i128(3),
        f.balance(&f.asset) - asset_before,
        "asset_out is what arrived"
    );
    assert_eq!(ev.i128(3), returned);
    assert_eq!(ev.data.len(), 4);
}

#[test]
fn exit_expired_reports_the_shares_actually_paid() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let lp = f.pool.balance_shares(&f.user);
    let pt = f.balance(&f.pt);
    f.accrue_yield(200_000_000);
    f.advance_past_maturity();
    let shares_before = f.balance(&f.vault);

    let shares_out = f
        .router
        .exit_expired(&f.vault, &f.maturity, &f.user, &lp, &1);
    let ev = last_router_event(&f);

    ev.assert_header(&f, "exit_expired");
    assert_eq!(ev.i128(1), lp, "lp_shares");
    assert!(
        ev.i128(2) >= pt,
        "pt_redeemed covers the PT held before the LP leg"
    );
    assert_eq!(
        ev.i128(3),
        f.balance(&f.vault) - shares_before,
        "shares_out is the net share gain"
    );
    assert_eq!(ev.i128(3), shares_out);
    assert_eq!(ev.data.len(), 4);
}
