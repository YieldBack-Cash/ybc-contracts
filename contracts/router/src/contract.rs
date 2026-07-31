use amm_interface::AmmClient;
use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, token, Address, Env,
};
use vault_interface::VaultContractClient;
use yield_manager_interface::YieldManagerClient;
use yield_token_interface::YieldTokenClient;

use crate::events::{ExitedExpired, ExitedExpiredToAsset, RoutedYtBuy, RoutedYtSell, ZappedIn, ZappedOut};
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
    //
    // Every parameter below is CALLER-CHOSEN, and that is the design: a user's
    // signed authorization on Soroban must match argument-for-argument at
    // execution, so nothing the chain computes (vault share counts, ledger
    // numbers, pool-priced amounts) may ever appear in one. Measured quantities
    // move instead under contract authority — the YM redeems its own custody,
    // the AMM pulls the caller's bound and refunds, and leftover shares are
    // swept by the router acting as vault OPERATOR against a caller-signed
    // allowance (`sweep_allowance` shares, expiring at ledger `sweep_expiry`).
    //
    // Bounds double as funding: a `max_*` bound is pulled in full and the
    // excess refunded, so the account must actually hold it at that leg.
    fn zap_asset_for_pt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        max_asset_in: i128,
        max_v_in: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128;
    fn zap_pt_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_asset_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128;
    fn zap_asset_for_yt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_asset_in: i128,
        max_v_in: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128;
    fn zap_yt_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_asset_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
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
        max_v_in: i128,
        desired_v: i128,
        min_lp_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128;
    fn zap_lp_for_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        pt_to_sell: i128,
        min_asset_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128;
    fn exit_expired_to_asset(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        max_pt: i128,
        pt_allow_expiry: u32,
        min_asset_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
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
/// `to` is the vault's `from`, `receiver` and `operator` alike, so the router
/// takes no custody and — because `assets` is caller-chosen — every entry in
/// the user's signed tree (this deposit and its nested asset transfer) is
/// drift-free by construction. The share count, which nobody can predict at
/// signing time, is only ever *measured* here, never signed.
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

/// Converts every vault share `to` gained since `shares_before` back into the
/// underlying — the cleanup ("sweep") that keeps a zap's promise that the user
/// never ends up holding shares. Returns the asset delivered; a zap that gained
/// nothing sweeps nothing.
///
/// The gained amount is freshly measured, so it cannot appear in the user's
/// signature. Instead the user signs a vault-share allowance whose arguments
/// they chose (`sweep_allowance` shares, dead at ledger `sweep_expiry`), and
/// the router — as SEP-56 OPERATOR — drives `redeem` with the measured figure,
/// consuming the allowance. Both vaults this runs against implement exactly
/// that operator semantics. The proceeds go straight to `to`; the router's
/// authority here is only to trigger the conversion, never to redirect it.
fn sweep_gained_shares(
    e: &Env,
    vault: &Address,
    asset: &Address,
    to: &Address,
    shares_before: i128,
    sweep_allowance: i128,
    sweep_expiry: u32,
) -> i128 {
    let vault_token = token::TokenClient::new(e, vault);
    let gained = vault_token.balance(to) - shares_before;
    if gained <= 0 {
        return 0;
    }
    assert!(
        gained <= sweep_allowance,
        "sweep_allowance below the shares this zap produced"
    );

    // User-signed, fixed-argument. The one thing a caller must NOT do is derive
    // `sweep_expiry` from the current ledger — that is exactly the
    // simulation/execution drift this design exists to eliminate.
    vault_token.approve(to, &e.current_contract_address(), &sweep_allowance, &sweep_expiry);

    let asset_token = token::TokenClient::new(e, asset);
    let assets_before = asset_token.balance(to);
    VaultContractClient::new(e, vault).redeem(&gained, to, to, &e.current_contract_address());
    let asset_out = asset_token.balance(to) - assets_before;

    ZappedOut {
        vault: vault.clone(),
        to: to.clone(),
        asset: asset.clone(),
        shares_in: gained,
        asset_out,
    }
    .publish(e);

    asset_out
}

/// Shared body of the share-denominated expired-market exit: burns the LP
/// position, redeems the user's whole PT balance and sweeps YT yield. Returns
/// `(vault shares gained, PT redeemed)`. Applies no slippage bound — the caller
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
    // user only ever touches the base asset. Four rules hold throughout:
    //
    //   * The router custodies nothing. Every leg names the user as the
    //     token-holding party, so a revert anywhere strands no funds and the
    //     "router holds no funds" property in ARCHITECTURE.md survives intact.
    //   * NOTHING THE CHAIN COMPUTES MAY ENTER A USER'S SIGNATURE. Soroban
    //     matches a signed authorization argument-for-argument at execution, and
    //     a wallet builds that signature by simulating beforehand — so a vault
    //     share count, a pool-priced amount or a current ledger number is a
    //     guaranteed mismatch. Measured values move under contract authority
    //     instead: the AMM pulls the caller's bound and refunds, the YM redeems
    //     its own custody, and leftovers are swept by the router as vault
    //     operator against a caller-signed allowance.
    //   * Slippage is bounded in base-asset terms at the endpoint, from measured
    //     balance deltas — one number covering pool price AND vault rate.
    //   * Nothing trusts a vault's self-reported amount. Every quantity crossing
    //     the vault boundary is measured, because SEP-56 leaves fees and
    //     rounding to the implementation.

    /// Buy exactly `pt_out` PT using the base asset. `max_asset_in` is deposited
    /// in full and `max_v_in` handed to the pool, which keeps only what the
    /// trade costs; everything left over is swept back to the asset. Returns the
    /// asset actually spent.
    fn zap_asset_for_pt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        max_asset_in: i128,
        max_v_in: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(pt_out > 0, "pt_out must be positive");
        assert!(max_asset_in > 0, "max_asset_in must be positive");
        assert!(max_v_in > 0, "max_v_in must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let asset_token = token::TokenClient::new(&e, &asset);
        let vault_token = token::TokenClient::new(&e, &vault);

        let asset_before = asset_token.balance(&to);
        let shares_before = vault_token.balance(&to);

        // Both bounds are the caller's own numbers, so both are signable. The
        // deposit's share yield is measured only to check the pool's bound can
        // actually be funded — it never reaches an auth entry.
        let shares_in = deposit_assets(&e, &vault, &asset, &to, max_asset_in);
        assert!(shares_in >= max_v_in, "deposit did not fund max_v_in");
        AmmClient::new(&e, &market.pool).swap_v_for_pt(&to, &pt_out, &max_v_in);

        sweep_gained_shares(&e, &vault, &asset, &to, shares_before, sweep_allowance, sweep_expiry);

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
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(pt_in > 0, "pt_in must be positive");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);

        let shares_before = vault_token.balance(&to);
        // `pt_in` is caller-chosen so the PT leg is signable as-is; the AMM
        // demands a positive share bound and 1 is the widest legal value, with
        // the real bound applied in asset terms below.
        AmmClient::new(&e, &market.pool).swap_pt_for_v(&to, &pt_in, &1);

        let asset_out = sweep_gained_shares(
            &e, &vault, &asset, &to, shares_before, sweep_allowance, sweep_expiry,
        );
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");
        asset_out
    }

    /// Buy exactly `yt_out` YT using the base asset. Returns the asset spent.
    fn zap_asset_for_yt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_asset_in: i128,
        max_v_in: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_out > 0, "yt_out must be positive");
        assert!(max_asset_in > 0, "max_asset_in must be positive");
        assert!(max_v_in > 0, "max_v_in must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let asset_token = token::TokenClient::new(&e, &asset);
        let vault_token = token::TokenClient::new(&e, &vault);

        let asset_before = asset_token.balance(&to);
        let shares_before = vault_token.balance(&to);

        // The YM's flash callback already pulls exactly `max_v_in` and refunds
        // the rest — the pull-the-bound pattern this whole design generalises.
        let shares_in = deposit_assets(&e, &vault, &asset, &to, max_asset_in);
        assert!(shares_in >= max_v_in, "deposit did not fund max_v_in");
        AmmClient::new(&e, &market.pool).flash_swap_pt(&market.ym, &yt_out, &to, &max_v_in);

        sweep_gained_shares(&e, &vault, &asset, &to, shares_before, sweep_allowance, sweep_expiry);

        let asset_spent = asset_before - asset_token.balance(&to);
        assert!(asset_spent <= max_asset_in, "vault took more than max_asset_in");

        RoutedYtBuy { vault, to, maturity, yt_out, max_v_in }.publish(&e);
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
        sweep_allowance: i128,
        sweep_expiry: u32,
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

        let asset_out = sweep_gained_shares(
            &e, &vault, &asset, &to, shares_before, sweep_allowance, sweep_expiry,
        );
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");

        RoutedYtSell { vault, to, maturity, yt_in, min_v_out: min_asset_out }.publish(&e);
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

        // Straight to the YM, which deposits into the vault with ITSELF as
        // receiver — the shares never touch the user's account, so there is no
        // allowance to grant and nothing measured to sign. An earlier revision
        // did the vault deposit here and then approved the YM for the resulting
        // share count, with an expiry read from the current ledger; both of
        // those are execution-time values, and it failed on testnet with
        // Auth/InvalidAction every single time.
        YieldManagerClient::new(&e, &market.ym).deposit_asset(&to, &asset_in, &min_tokens_out)
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

        // The YM burns the pair (both amounts caller-chosen, so signable) and
        // redeems the owed shares from its OWN custody, with the vault paying
        // the user directly. No share count reaches the user's signature.
        YieldManagerClient::new(&e, &market.ym)
            .redeem_combined_to_asset(&to, &amount, &min_asset_out)
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
    /// Leftover vault shares are swept back to the base asset. Leftover PT stays
    /// with the user: selling a dust amount back into the pool can trip the AMM's
    /// positive-amount asserts and revert an otherwise-good transaction, and PT
    /// is a token the user may well want to keep anyway.
    ///
    /// `desired_v` is the share side offered to the pool. Like `pt_to_buy` it is
    /// the caller's figure — the pool pulls it in full and refunds what its ratio
    /// declines, so it must be signable rather than measured.
    fn zap_asset_for_lp(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        asset_in: i128,
        pt_to_buy: i128,
        max_v_in: i128,
        desired_v: i128,
        min_lp_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(asset_in > 0, "asset_in must be positive");
        assert!(pt_to_buy >= 0, "pt_to_buy must be non-negative");
        assert!(desired_v > 0, "desired_v must be positive");
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
            assert!(shares_in >= max_v_in, "deposit did not fund max_v_in");
            pool.swap_v_for_pt(&to, &pt_to_buy, &max_v_in);
        }

        // Offer the caller's own figures on both legs. The pool takes them in
        // its ratio and refunds the rest, so per-leg mins stay 0 and `min_lp_out`
        // is the real bound. PT bought is exactly `pt_to_buy`, so that side is
        // known without measuring.
        assert!(
            pt_token.balance(&to) - pt_before >= pt_to_buy,
            "PT leg short of pt_to_buy"
        );
        pool.deposit(&to, &pt_to_buy, &0, &desired_v, &0);

        let lp_out = pool.balance_shares(&to) - lp_before;
        assert!(lp_out >= min_lp_out, "min_lp_out not satisfied");

        sweep_gained_shares(&e, &vault, &asset, &to, shares_before, sweep_allowance, sweep_expiry);
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
    /// `pt_to_sell` is the caller's figure for how much PT to sell back into the
    /// pool — typically what the frontend simulated the withdrawal will yield.
    /// It cannot be measured on-chain and then sold, because the sale amount
    /// would land in the user's signature. Anything the withdrawal produces
    /// beyond it stays with the user; conversely the caller may deliberately
    /// include PT they already held, since they are stating intent rather than
    /// having it inferred.
    fn zap_lp_for_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        pt_to_sell: i128,
        min_asset_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(lp_shares > 0, "lp_shares must be positive");
        assert!(pt_to_sell >= 0, "pt_to_sell must be non-negative");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        let asset = vault_asset(&e, &vault);
        let vault_token = token::TokenClient::new(&e, &vault);
        let pt_token = token::TokenClient::new(&e, &market.pt);
        let pool = AmmClient::new(&e, &market.pool);

        let shares_before = vault_token.balance(&to);

        // Per-leg mins stay 0; the asset-denominated bound at the end is the
        // real slippage check.
        pool.withdraw(&to, &lp_shares, &0, &0);

        if pt_to_sell > 0 {
            assert!(
                pt_token.balance(&to) >= pt_to_sell,
                "PT balance short of pt_to_sell"
            );
            pool.swap_pt_for_v(&to, &pt_to_sell, &1);
        }

        let asset_out = sweep_gained_shares(
            &e, &vault, &asset, &to, shares_before, sweep_allowance, sweep_expiry,
        );
        assert!(asset_out >= min_asset_out, "min_asset_out not satisfied");
        asset_out
    }

    /// Base-asset counterpart of `exit_expired`: unwinds the LP position, all
    /// PT and any accrued YT yield, then returns the whole proceeds as the base
    /// asset. One signature takes an expired position all the way back to what
    /// the user started with. Returns the asset delivered.
    ///
    /// `max_pt` is a caller-chosen ceiling on the PT to redeem, granted to the
    /// YM as an allowance expiring at `pt_allow_expiry`. The YM burns
    /// `min(max_pt, balance)`, so "redeem everything, including the PT this
    /// withdrawal just produced" works without the measured figure ever entering
    /// the user's signature. Set `max_pt` generously; unused allowance is never
    /// taken and dies at the expiry.
    fn exit_expired_to_asset(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        max_pt: i128,
        pt_allow_expiry: u32,
        min_asset_out: i128,
        sweep_allowance: i128,
        sweep_expiry: u32,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(lp_shares >= 0, "lp_shares must be non-negative");
        assert!(max_pt > 0, "max_pt must be positive");
        assert!(sweep_allowance > 0, "sweep_allowance must be positive");
        assert!(min_asset_out > 0, "min_asset_out must be positive");

        let market = resolve_market(&e, &vault, maturity);
        assert!(
            e.ledger().timestamp() >= market.maturity,
            "market not expired"
        );

        // Order matters. Both of these pay the user in vault shares, and they
        // must land BEFORE the yield manager gathers them up, so the whole exit
        // settles in a single vault redemption.
        if lp_shares > 0 {
            AmmClient::new(&e, &market.pool).withdraw(&to, &lp_shares, &0, &0);
        }
        YieldTokenClient::new(&e, &market.yt).claim_yield(&to);

        // Two ceilings, both caller-chosen and therefore signable; the YM takes
        // only what is actually there. This is what lets freshly measured
        // amounts — the LP payout, the yield claim — be converted without ever
        // appearing in the user's signature.
        //
        // `sweep_allowance` is a ceiling on the user's WHOLE share balance, not
        // just what this call produced. The YM authenticates `from`, so the
        // figure it takes must be one the user signed in advance, which rules
        // out a measured "gained since we started". Size it to the expected
        // proceeds rather than passing something arbitrarily large.
        token::TokenClient::new(&e, &market.pt).approve(&to, &market.ym, &max_pt, &pt_allow_expiry);
        token::TokenClient::new(&e, &vault).approve(
            &to,
            &market.ym,
            &sweep_allowance,
            &sweep_expiry,
        );

        // One call, one redemption, covering the PT face value and every loose
        // share together. Redeeming those separately is what pushed this past
        // the transaction budget whenever an LP position was involved.
        let asset_out = YieldManagerClient::new(&e, &market.ym)
            .exit_expired_to_asset(&to, &max_pt, &sweep_allowance, &min_asset_out);

        ExitedExpiredToAsset {
            vault,
            to,
            maturity,
            lp_shares,
            asset_out,
        }
        .publish(&e);

        asset_out
    }
}
