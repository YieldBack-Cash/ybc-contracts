use amm_interface::AmmClient;
use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, token, Address, Env,
};
use vault_interface::VaultContractClient;
use yield_manager_interface::YieldManagerClient;
use yield_token_interface::YieldTokenClient;

use crate::events::{ExitedExpired, RoutedYtBuy, RoutedYtSell, ZappedIn, ZappedOut};
use crate::storage::{extend_instance_ttl, get_factory, set_factory};

/// Mirror of the factory's `Market` record. Field names and types must match
/// the factory's struct exactly so its return value decodes into this one.
#[contracttype]
#[derive(Clone)]
pub struct Market {
    pub ym: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: Address,
    pub maturity: u64,
    pub vault: Address,
}

/// Minimal view of the factory: just enough to resolve a single market.
/// The trait itself is never implemented or called — it exists only as the
/// source for the generated FactoryViewClient, which dead_code can't see.
#[allow(dead_code)]
#[contractclient(name = "FactoryViewClient")]
pub trait FactoryView {
    fn get_market(env: Env, vault: Address, maturity: u64) -> Option<Market>;
}

#[contractclient(name = "RouterClient")]
pub trait RouterInterface {
    fn get_amm(env: Env, vault: Address, maturity: u64) -> Address;
    fn swap_v_for_pt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        v_in_max: i128,
    );
    fn swap_pt_for_v(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_v_out: i128,
    );
    fn swap_v_for_yt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_v_in: i128,
    );
    fn swap_yt_for_v(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_v_out: i128,
    );
    fn deposit(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    );
    fn withdraw(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128);
    fn exit_expired(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_shares_out: i128,
    ) -> i128;
    fn get_reserves(env: Env, vault: Address, maturity: u64) -> (i128, i128);
    fn balance_shares(env: Env, vault: Address, maturity: u64, user: Address) -> i128;

    // ── Zaps: enter and leave a market holding only the base asset ───────────
    fn zap_asset_for_pt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        max_asset_in: i128,
    ) -> i128;
    fn zap_pt_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_asset_out: i128,
    ) -> i128;
    fn zap_asset_for_yt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_asset_in: i128,
    ) -> i128;
    fn zap_yt_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_asset_out: i128,
    ) -> i128;
    fn zap_asset_for_split(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        asset_in: i128,
        min_tokens_out: i128,
    ) -> i128;
    fn zap_split_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        amount: i128,
        min_asset_out: i128,
    ) -> i128;
    fn zap_asset_for_lp(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        asset_in: i128,
        pt_to_buy: i128,
        min_lp_out: i128,
    ) -> i128;
    fn zap_lp_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_asset_out: i128,
    ) -> i128;
    fn exit_expired_to_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_asset_out: i128,
    ) -> i128;
}

#[contract]
pub struct RouterContract;

/// Resolves a market by (vault, maturity) through the factory, so callers can't
/// point the router at a pool the factory didn't deploy. The factory keys each
/// market directly by (vault, maturity) and forbids overwriting it, so this is a
/// single O(1) lookup rather than a scan of the vault's whole market history.
fn resolve_market(e: &Env, vault: &Address, maturity: u64) -> Market {
    FactoryViewClient::new(e, &get_factory(e))
        .get_market(vault, &maturity)
        .expect("no market for vault and maturity")
}

/// The vault's underlying asset, per SEP-56. Resolve this ONCE per invocation
/// and thread the result through every leg: a vault that named one asset on the
/// way in and another on the way out could otherwise be paid in the valuable
/// token and settle in a worthless one.
fn vault_asset(e: &Env, vault: &Address) -> Address {
    VaultContractClient::new(e, vault).query_asset()
}

/// Deposits `assets` of the underlying from `to` into the vault, crediting the
/// shares to `to`. Returns the shares actually received.
///
/// `to` is the vault's `from`, `receiver` and `operator` alike — the router
/// never takes custody, so a revert in a later leg strands nothing, and there is
/// no invocation in which the router holds user funds.
///
/// **Passing one address for every role is load-bearing, not incidental.** SEP-56
/// leaves the auth pattern to implementers, and both vaults this runs against
/// (OpenZeppelin's and Blend's) delegate the same way: when `operator` differs
/// from the funds-owner, they require and CONSUME a SEP-41 allowance rather than
/// a signature. Same address means one auth and no allowance. Anything that made
/// the router act as `operator` on a user's behalf — a custody model, a batching
/// layer — would fail on the missing allowance until the user separately called
/// `approve` on the vault. `redeem_shares` carries the identical constraint.
///
/// The count comes from a measured balance delta, not the vault's return value:
/// SEP-56 says nothing about fees, so a vault may mint fewer shares than an
/// idealized conversion implies.
fn deposit_assets(e: &Env, vault: &Address, asset: &Address, to: &Address, assets: i128) -> i128 {
    let vault_token = token::TokenClient::new(e, vault);
    let shares_before = vault_token.balance(to);
    VaultContractClient::new(e, vault).deposit(&assets, to, to, to);
    let shares_out = vault_token.balance(to) - shares_before;
    assert!(shares_out > 0, "vault minted no shares");

    ZappedIn {
        vault: vault.clone(),
        to: to.clone(),
        asset: asset.clone(),
        asset_in: assets,
        shares_out,
    }
    .publish(e);

    shares_out
}

/// Redeems `shares` held by `to` back into the underlying, paid to `to`.
/// Returns the assets actually received; a non-positive `shares` is a no-op so
/// callers can pass an unspent remainder without branching.
///
/// Share-denominated `redeem` rather than asset-denominated `withdraw`: the
/// caller always knows exactly how many shares it wants gone, and asking in
/// share terms needs no preview round-trip and leaves no dust behind.
fn redeem_shares(e: &Env, vault: &Address, asset: &Address, to: &Address, shares: i128) -> i128 {
    if shares <= 0 {
        return 0;
    }
    let asset_token = token::TokenClient::new(e, asset);
    let vault_token = token::TokenClient::new(e, vault);
    let assets_before = asset_token.balance(to);
    let shares_before = vault_token.balance(to);

    VaultContractClient::new(e, vault).redeem(&shares, to, to, to);

    // One-sided on purpose. Burning MORE than asked is the vault helping itself
    // to shares it did not pay for, so it must revert. Burning less only leaves
    // the user holding a share they would rather have converted — cosmetic, and
    // reverting the whole exit over it would turn a wart into a standing outage
    // for every user of that vault.
    //
    // That the burn is *exact* against a well-behaved vault is a property of the
    // router, and the zap tests assert it there; it is not something to enforce
    // on-chain against a vault we do not control.
    assert!(
        shares_before - vault_token.balance(to) <= shares,
        "vault burned more shares than requested"
    );

    let asset_out = asset_token.balance(to) - assets_before;

    ZappedOut {
        vault: vault.clone(),
        to: to.clone(),
        asset: asset.clone(),
        shares_in: shares,
        asset_out,
    }
    .publish(e);

    asset_out
}

/// Shared body of both expired-market exits: burns the LP position, redeems the
/// user's whole PT balance and sweeps YT yield. Returns
/// `(vault shares gained, PT redeemed)`. Applies no slippage bound — each caller
/// denominates that in the unit it settles in.
fn unwind_expired(e: &Env, market: &Market, to: &Address, lp_shares: i128) -> (i128, i128) {
    assert!(
        e.ledger().timestamp() >= market.maturity,
        "market not expired"
    );

    let vault_token = token::TokenClient::new(e, &market.vault);
    let shares_before = vault_token.balance(to);

    // The pool pays out both legs (PT + vault shares) directly to the user.
    // Per-leg mins stay 0: PT converts at the YM's fixed rate below, so the
    // caller's aggregate bound is the real slippage check.
    if lp_shares > 0 {
        AmmClient::new(e, &market.pool).withdraw(to, &lp_shares, &0, &0);
    }

    // Redeem the full PT balance — the LP-withdrawn PT plus any the user
    // already held. Post-maturity PT has no other use.
    let pt_balance = token::TokenClient::new(e, &market.pt).balance(to);
    if pt_balance > 0 {
        YieldManagerClient::new(e, &market.ym).redeem_principal(to, &pt_balance);
    }

    YieldTokenClient::new(e, &market.yt).claim_yield(to);

    (vault_token.balance(to) - shares_before, pt_balance)
}

#[contractimpl]
impl RouterContract {
    pub fn __constructor(e: Env, factory: Address) {
        set_factory(&e, &factory);
    }
}

#[contractimpl]
impl RouterInterface for RouterContract {
    fn get_amm(e: Env, vault: Address, maturity: u64) -> Address {
        extend_instance_ttl(&e);
        resolve_market(&e, &vault, maturity).pool
    }

    fn swap_v_for_pt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        v_in_max: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).swap_v_for_pt(&to, &pt_out, &v_in_max);
    }

    fn swap_pt_for_v(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_v_out: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).swap_pt_for_v(&to, &pt_in, &min_v_out);
    }

    fn swap_v_for_yt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_v_in: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_out > 0, "yt_out must be positive");
        assert!(max_v_in > 0, "max_v_in must be positive");

        let market = resolve_market(&e, &vault, maturity);

        // Flash swap: the YM mints yt_out PT+YT, keeps the PT for the pool, and gives
        // the user the YT. The user pays only the YT price, bounded by max_v_in.
        AmmClient::new(&e, &market.pool).flash_swap_pt(&market.ym, &yt_out, &to, &max_v_in);

        RoutedYtBuy {
            vault,
            to,
            maturity,
            yt_out,
            max_v_in,
        }
        .publish(&e);
    }

    fn swap_yt_for_v(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_v_out: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_in > 0, "yt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        let market = resolve_market(&e, &vault, maturity);

        // Transfer the YT before the flash swap so the user's signed auth entry is a
        // plain transfer with fixed args — nothing that drifts with pool state.
        token::TokenClient::new(&e, &market.yt).transfer(&to, &market.ym, &yt_in);

        // Borrow exactly `yt_in` PT so it pairs 1:1 with the YT now held by the YM.
        // The YM is the callback receiver — it burns PT+YT and repays the AMM.
        AmmClient::new(&e, &market.pool).flash_swap_v(&market.ym, &yt_in, &to, &min_v_out);

        RoutedYtSell {
            vault,
            to,
            maturity,
            yt_in,
            min_v_out,
        }
        .publish(&e);
    }

    fn deposit(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).deposit(&to, &desired_a, &min_a, &desired_b, &min_b);
    }

    fn withdraw(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).withdraw(&to, &share_amount, &min_a, &min_b)
    }

    /// One-call exit from an expired market: burns the user's LP position,
    /// redeems their entire PT balance at the YM's locked post-maturity rate,
    /// and sweeps yield the YT accrued before maturity — everything lands as
    /// vault shares. `min_shares_out` bounds the total delivered.
    fn exit_expired(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_shares_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(lp_shares >= 0, "lp_shares must be non-negative");

        let market = resolve_market(&e, &vault, maturity);
        let (shares_out, pt_redeemed) = unwind_expired(&e, &market, &to, lp_shares);
        assert!(shares_out >= min_shares_out, "min_shares_out not satisfied");

        ExitedExpired {
            vault,
            to,
            maturity,
            lp_shares,
            pt_redeemed,
            shares_out,
        }
        .publish(&e);

        shares_out
    }

    fn get_reserves(e: Env, vault: Address, maturity: u64) -> (i128, i128) {
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).get_reserves()
    }

    fn balance_shares(e: Env, vault: Address, maturity: u64, user: Address) -> i128 {
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).balance_shares(&user)
    }

    // ── Zaps ────────────────────────────────────────────────────────────────
    //
    // Each zap wraps an existing operation in a vault deposit or redeem so the
    // user only ever touches the base asset. Three rules hold throughout:
    //
    //   * The router still custodies nothing. Every leg names the user as the
    //     token-holding party, so a revert anywhere strands no funds and the
    //     "router holds no funds" property in ARCHITECTURE.md survives intact.
    //   * Slippage is bounded once, at the endpoint, from measured balance
    //     deltas. Per-leg share bounds are deliberately left wide: a single
    //     asset-denominated number covers pool price AND vault rate together,
    //     and it is the only figure the user actually cares about. Intermediate
    //     bounds would add ways to fail without adding protection.
    //   * Nothing trusts a vault's self-reported amount. Every quantity that
    //     crosses the vault boundary is measured before and after, because
    //     SEP-56 leaves fees and rounding to the implementation.

    /// Buy exactly `pt_out` PT using the base asset, spending at most
    /// `max_asset_in`. Whatever the swap does not consume goes back through the
    /// vault, so the user ends holding only PT and the base asset. Returns the
    /// asset actually spent.
    fn zap_asset_for_pt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        max_asset_in: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(pt_out > 0, "pt_out must be positive");
        assert!(max_asset_in > 0, "max_asset_in must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let asset_token = token::TokenClient::new(&e, &asset);
        let vault_token = token::TokenClient::new(&e, &vault);

        let asset_before = asset_token.balance(&to);
        let shares_before = vault_token.balance(&to);

        let shares_in = deposit_assets(&e, &vault, &asset, &to, max_asset_in);
        AmmClient::new(&e, &market.pool).swap_v_for_pt(&to, &pt_out, &shares_in);

        // Unwind whatever the swap didn't need. Measured against the pre-zap
        // balance, so shares the user already held are left untouched.
        let unspent = vault_token.balance(&to) - shares_before;
        redeem_shares(&e, &vault, &asset, &to, unspent);

        // Not the slippage bound — the AMM leg above already rejects a trade it
        // can't fill within the shares it was handed. This catches a vault that
        // pulls more of the asset than the deposit asked for.
        let asset_spent = asset_before - asset_token.balance(&to);
        assert!(asset_spent <= max_asset_in, "vault took more than max_asset_in");
        asset_spent
    }

    /// Sell exactly `pt_in` PT and leave holding the base asset. Returns the
    /// asset delivered, which must be at least `min_asset_out`.
    fn zap_pt_for_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_asset_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(pt_in > 0, "pt_in must be positive");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);

        let shares_before = vault_token.balance(&to);
        // The AMM demands a positive share bound; 1 is the widest legal value.
        AmmClient::new(&e, &market.pool).swap_pt_for_v(&to, &pt_in, &1);
        let shares_out = vault_token.balance(&to) - shares_before;

        let asset_out = redeem_shares(&e, &vault, &asset, &to, shares_out);
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");
        asset_out
    }

    /// Buy exactly `yt_out` YT using the base asset, spending at most
    /// `max_asset_in`. Returns the asset actually spent.
    fn zap_asset_for_yt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_asset_in: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_out > 0, "yt_out must be positive");
        assert!(max_asset_in > 0, "max_asset_in must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let asset_token = token::TokenClient::new(&e, &asset);
        let vault_token = token::TokenClient::new(&e, &vault);

        let asset_before = asset_token.balance(&to);
        let shares_before = vault_token.balance(&to);

        // The YM pulls the full bound during its callback and refunds the
        // excess, so handing it every share the deposit produced costs nothing.
        let shares_in = deposit_assets(&e, &vault, &asset, &to, max_asset_in);
        AmmClient::new(&e, &market.pool).flash_swap_pt(&market.ym, &yt_out, &to, &shares_in);

        let unspent = vault_token.balance(&to) - shares_before;
        redeem_shares(&e, &vault, &asset, &to, unspent);

        // Not the slippage bound — the AMM leg above already rejects a trade it
        // can't fill within the shares it was handed. This catches a vault that
        // pulls more of the asset than the deposit asked for.
        let asset_spent = asset_before - asset_token.balance(&to);
        assert!(asset_spent <= max_asset_in, "vault took more than max_asset_in");

        RoutedYtBuy {
            vault,
            to,
            maturity,
            yt_out,
            max_v_in: shares_in,
        }
        .publish(&e);

        asset_spent
    }

    /// Sell exactly `yt_in` YT and leave holding the base asset. Returns the
    /// asset delivered, which must be at least `min_asset_out`.
    fn zap_yt_for_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_asset_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_in > 0, "yt_in must be positive");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);

        let shares_before = vault_token.balance(&to);

        // Same ordering as swap_yt_for_v: move the YT first so the user's signed
        // entry is a fixed-arg transfer, then let the YM drive the redeem.
        token::TokenClient::new(&e, &market.yt).transfer(&to, &market.ym, &yt_in);
        AmmClient::new(&e, &market.pool).flash_swap_v(&market.ym, &yt_in, &to, &1);

        let shares_out = vault_token.balance(&to) - shares_before;
        let asset_out = redeem_shares(&e, &vault, &asset, &to, shares_out);
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");

        RoutedYtSell {
            vault,
            to,
            maturity,
            yt_in,
            min_v_out: shares_out,
        }
        .publish(&e);

        asset_out
    }

    /// Split the base asset straight into PT + YT: deposit into the vault, then
    /// mint through the yield manager. Returns the amount of each token minted
    /// (PT and YT always mint in equal measure), bounded by `min_tokens_out`.
    fn zap_asset_for_split(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        asset_in: i128,
        min_tokens_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(asset_in > 0, "asset_in must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let pt_token = token::TokenClient::new(&e, &market.pt);

        let pt_before = pt_token.balance(&to);
        let shares_in = deposit_assets(&e, &vault, &asset, &to, asset_in);

        // The YM takes custody with `transfer_from` as spender, so it needs an
        // allowance. Sized to exactly this deposit and expiring at the end of
        // the current ledger, so no spending power outlives the call.
        token::TokenClient::new(&e, &vault).approve(
            &to,
            &market.ym,
            &shares_in,
            &e.ledger().sequence(),
        );
        YieldManagerClient::new(&e, &market.ym).deposit(&to, &shares_in);

        let minted = pt_token.balance(&to) - pt_before;
        assert!(minted >= min_tokens_out, "min_tokens_out not satisfied");
        minted
    }

    /// Recombine `amount` of PT + YT back into the base asset before maturity.
    /// Returns the asset delivered, at least `min_asset_out`.
    fn zap_split_for_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        amount: i128,
        min_asset_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(amount > 0, "amount must be positive");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);

        let shares_before = vault_token.balance(&to);
        YieldManagerClient::new(&e, &market.ym).redeem_combined(&to, &amount);
        let shares_out = vault_token.balance(&to) - shares_before;

        let asset_out = redeem_shares(&e, &vault, &asset, &to, shares_out);
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");
        asset_out
    }

    /// Provide liquidity starting from the base asset alone: deposit `asset_in`
    /// into the vault, buy `pt_to_buy` PT with part of the proceeds, then add
    /// both legs to the pool. Returns the LP shares minted, at least
    /// `min_lp_out`.
    ///
    /// `pt_to_buy` is the caller's to choose. The pool only accepts the two legs
    /// in its current reserve ratio, and finding the split that lands exactly on
    /// that ratio means solving against the curve. Rather than duplicate the
    /// AMM's math here (and drift from it), the frontend simulates for
    /// `pt_to_buy` and the router refunds whatever the pool declines — so a
    /// slightly wrong number costs a small refund, not a failed transaction.
    ///
    /// Leftover vault shares go back to the base asset. Leftover PT stays with
    /// the user: selling a dust amount back into the pool can trip the AMM's
    /// positive-amount asserts and revert an otherwise-good transaction, and PT
    /// is a token the user may well want to keep anyway.
    fn zap_asset_for_lp(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        asset_in: i128,
        pt_to_buy: i128,
        min_lp_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(asset_in > 0, "asset_in must be positive");
        assert!(pt_to_buy >= 0, "pt_to_buy must be non-negative");
        assert!(min_lp_out > 0, "min_lp_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);
        let pt_token = token::TokenClient::new(&e, &market.pt);
        let pool = AmmClient::new(&e, &market.pool);

        let shares_before = vault_token.balance(&to);
        let pt_before = pt_token.balance(&to);
        let lp_before = pool.balance_shares(&to);

        let shares_in = deposit_assets(&e, &vault, &asset, &to, asset_in);
        if pt_to_buy > 0 {
            pool.swap_v_for_pt(&to, &pt_to_buy, &shares_in);
        }

        // Offer only what this zap produced, never balances the user already
        // held. The AMM takes the legs in its own ratio and leaves the rest, so
        // the per-leg mins stay 0 — `min_lp_out` below is the real bound.
        let pt_available = pt_token.balance(&to) - pt_before;
        let shares_available = vault_token.balance(&to) - shares_before;
        assert!(
            pt_available > 0 && shares_available > 0,
            "both legs must be positive to add liquidity"
        );
        pool.deposit(&to, &pt_available, &0, &shares_available, &0);

        let lp_out = pool.balance_shares(&to) - lp_before;
        assert!(lp_out >= min_lp_out, "min_lp_out not satisfied");

        let unspent = vault_token.balance(&to) - shares_before;
        redeem_shares(&e, &vault, &asset, &to, unspent);

        lp_out
    }

    /// Withdraw an LP position and leave holding the base asset: burn
    /// `lp_shares`, sell the PT leg back into the pool, and redeem the whole
    /// proceeds through the vault. Returns the asset delivered, at least
    /// `min_asset_out`.
    ///
    /// Selling the PT leg into the same pool it just came out of moves the price
    /// against the seller, and the effect grows with position size — a large
    /// exit realises noticeably less than the position's quoted value. For an
    /// expired market use `exit_expired_to_asset` instead, where PT redeems at
    /// par through the YM and no swap is needed.
    fn zap_lp_for_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_asset_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(lp_shares > 0, "lp_shares must be positive");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);
        let pt_token = token::TokenClient::new(&e, &market.pt);
        let pool = AmmClient::new(&e, &market.pool);

        let shares_before = vault_token.balance(&to);
        let pt_before = pt_token.balance(&to);

        // Per-leg mins stay 0; the asset-denominated bound at the end is the
        // real slippage check.
        pool.withdraw(&to, &lp_shares, &0, &0);

        // Sell only the PT this withdrawal produced — never PT the user already
        // held for other reasons.
        let pt_out = pt_token.balance(&to) - pt_before;
        if pt_out > 0 {
            pool.swap_pt_for_v(&to, &pt_out, &1);
        }

        let shares_out = vault_token.balance(&to) - shares_before;
        let asset_out = redeem_shares(&e, &vault, &asset, &to, shares_out);
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");
        asset_out
    }

    /// Base-asset counterpart of `exit_expired`: unwinds the LP position, all
    /// PT and any accrued YT yield, then redeems the whole proceeds through the
    /// vault. One signature takes an expired position all the way back to the
    /// asset the user started with. Returns the asset delivered.
    fn exit_expired_to_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_asset_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(lp_shares >= 0, "lp_shares must be non-negative");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);

        let (shares_out, pt_redeemed) = unwind_expired(&e, &market, &to, lp_shares);
        let asset_out = redeem_shares(&e, &vault, &asset, &to, shares_out);
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");

        ExitedExpired {
            vault,
            to,
            maturity,
            lp_shares,
            pt_redeemed,
            shares_out,
        }
        .publish(&e);

        asset_out
    }
}
