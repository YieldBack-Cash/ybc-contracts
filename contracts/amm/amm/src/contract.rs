use crate::curve::{calc_trade, compute_rate_anchor, get_exchange_rate_from_trade};
use crate::events::{Deposit, FlashSwapPt, FlashSwapV, PoolInit, ReserveFeePaid, SwapPtForV, SwapVForPt, Withdraw};
use crate::transfers::{get_deposit_amounts, transfer_pt_from_pool_to_user, transfer_pt_from_user_to_pool, transfer_v_from_user_to_pool, transfer_v_from_pool_to_user};
use crate::vault::VaultRate;
use crate::storage::*;
use num_integer::Roots;
use amm_interface::{AmmError, AmmInterface, FlashSwapPtReceiverClient, FlashSwapVReceiverClient};
use soroban_sdk::{contract, contractimpl, panic_with_error, token, Address, Env};

const MINIMUM_LIQUIDITY: i128 = 100;
const BURN_ADDRESS: &str = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

/// Bounds on creator-supplied market parameters (all 1e7-scaled APYs).
/// Outside these ranges the market is degenerate: a band narrower than
/// MIN_BAND_WIDTH makes the curve so steep it rejects almost every trade,
/// an APY above MAX_APY pushes `e^(rate·t)` outside the range where the
/// fixed-point exp/ln approximations are accurate, and a fee above
/// MAX_FEE_APY makes trading pointless.
const MAX_APY: i128 = 10_000_000; // 100%
const MIN_BAND_WIDTH: i128 = 100_000; // 1 percentage point
const MAX_FEE_APY: i128 = 200_000; // 2%

/// Cap on the treasury's share of the trading fee (1e7-scaled fraction of the
/// fee, not of the trade). Above 50% the LP cut stops being worth providing
/// liquidity for. calc_trade itself accepts the full 0–100% range; this is
/// the policy bound enforced at construction.
const MAX_RESERVE_FEE_RATE: i128 = 5_000_000; // 50% of the fee

/// ln(9), 1e7-scaled: the curve's logit term ln(p/(1-p)) at the p = 0.9 and
/// p = 0.1 proportions where the APY band edges are pinned.
const LN_9: i128 = 21_972_246;

#[contract]
pub struct LiquidityPool;

#[contractimpl]
impl LiquidityPool {
    /// Initializes the pool. Curve parameters are derived from
    /// APY-denominated inputs (1e7-scaled, e.g. 500_000 = 5%).
    ///
    /// # Arguments
    /// * `token_a` - First token address (must be < `token_b`)
    /// * `token_b` - Second token address (vault share token)
    /// * `expiry_ts` - Unix timestamp at which the market expires
    /// * `current_apy` - APY the market opens trading at
    /// * `apy_min` / `apy_max` - band the curve is tuned to trade within: the
    ///   implied rate reaches `apy_max` when the pool is 90% PT and `apy_min`
    ///   at 10% PT. Soft edges — the hard limits are the proportion bounds.
    /// * `fee_apy` - fee as an annualized rate spread, decays to zero at expiry
    /// * `ym` - Trusted yield manager; the only address accepted as a flash-swap receiver
    /// * `treasury` - Protocol fee sink; receives the reserve cut of each trade's fee
    /// * `reserve_fee_rate` - Treasury's share of the fee (1e7-scaled, e.g.
    ///   `1_000_000` = 10% of the fee). Immutable once set — there is no setter.
    pub fn __constructor(
        e: Env,
        token_a: Address,
        token_b: Address,
        expiry_ts: u64,
        current_apy: i128,
        apy_min: i128,
        apy_max: i128,
        fee_apy: i128,
        ym: Address,
        treasury: Address,
        reserve_fee_rate: i128,
    ) {
        let now = e.ledger().timestamp();
        if expiry_ts <= now {
            panic_with_error!(&e, AmmError::ExpiryNotInFuture);
        }
        if apy_min < 0 || !(apy_min < current_apy && current_apy < apy_max) {
            panic_with_error!(&e, AmmError::InvalidApyBand);
        }
        if apy_max > MAX_APY {
            panic_with_error!(&e, AmmError::ApyMaxTooHigh);
        }
        if apy_max - apy_min < MIN_BAND_WIDTH {
            panic_with_error!(&e, AmmError::BandTooNarrow);
        }
        if fee_apy <= 0 || fee_apy > MAX_FEE_APY {
            panic_with_error!(&e, AmmError::FeeApyOutOfRange);
        }
        if reserve_fee_rate < 0 || reserve_fee_rate > MAX_RESERVE_FEE_RATE {
            panic_with_error!(&e, AmmError::ReserveFeeRateOutOfRange);
        }

        // The curve stores rates in ln space (exchange_rate = e^(rate·t)), so
        // an APY maps to ln(1 + apy). The band collapses into curve steepness:
        // at the p = 0.9 / 0.1 pins the logit term is ±ln(9), and to first
        // order the resulting APY half-width ln(9)/scalar_root is the same at
        // any time to expiry.
        let ln_or_panic = |x: i128| {
            crate::math::ln_fp(crate::math::FP_SCALE + x, crate::math::FP_SCALE)
                .unwrap_or_else(|err| panic_with_error!(&e, err))
        };
        let last_implied_rate = ln_or_panic(current_apy);
        let fee_rate_root = ln_or_panic(fee_apy);
        let scalar_root = crate::math::div_down(2 * LN_9, apy_max - apy_min);

        set_ym(&e, &ym);
        set_treasury(&e, &treasury);
        set_reserve_fee_rate(&e, reserve_fee_rate);

        put_market_state(&e, &MarketState {
            token_a: token_a.clone(),
            token_b: token_b.clone(),
            reserve_a: 0,
            reserve_b: 0,
            expiry_ts,
            last_implied_rate,
            scalar_root,
            fee_rate_root,
        });
        put_total_shares(&e, 0);

        PoolInit {
            token_a,
            token_b,
            expiry_ts,
            current_apy,
            apy_min,
            apy_max,
            fee_apy,
            scalar_root,
            fee_rate_root,
            last_implied_rate,
            treasury,
            reserve_fee_rate,
        }
        .publish(&e);
    }
}

// ── shared by the four trade entry points ───────────────────────────────────

/// Converts a fee amount (the whole fee, or its reserve cut) from asset
/// units to vault shares. Floors (on top of the floored split in
/// calc_trade), so the rounding dust stays with the LPs; returns 0 for a
/// non-positive amount.
fn fee_in_shares(rate: &VaultRate, fee_assets: i128) -> Result<i128, AmmError> {
    if fee_assets <= 0 {
        return Ok(0);
    }
    Ok(rate.to_shares(fee_assets)?.max(0))
}

/// Remits an already-converted reserve-fee cut to the treasury and emits
/// `ReserveFeePaid`. No-op for zero, so a zero-rate market never touches
/// the treasury. `Pricing::settle` subtracts the same amount from
/// `reserve_b`, so the cut never enters LP accounting.
fn remit_reserve_fee(e: &Env, token_b: &Address, fee_shares: i128) {
    if fee_shares <= 0 {
        return;
    }
    let treasury = get_treasury(e);
    token::TokenClient::new(e, token_b)
        .transfer(&e.current_contract_address(), &treasury, &fee_shares);
    ReserveFeePaid { treasury, amount: fee_shares }.publish(e);
}

/// Everything a trade prices against, read once at the top of every trade
/// entry point: the market, the vault rate, and the curve's parameters at
/// this moment in the market's life. Built by `load`, priced by `quote`,
/// closed by `settle`; the four entry points differ only in the transfers
/// (and, for the flash swaps, the callback) they run between those calls.
struct Pricing {
    market: MarketState,
    rate: VaultRate,
    /// The V reserve in asset units, which is what the curve prices in.
    reserve_b_assets: i128,
    /// Time to expiry, 1e7-scaled years; positive by construction.
    years: i128,
    rate_scalar: i128,
    rate_anchor: i128,
    fee_factor: i128,
    reserve_fee_rate: i128,
}

/// A priced trade: the V that changes hands, and the two fee figures every
/// trade event reports.
struct Quote {
    /// Pendle's sign convention, in asset units: positive means V flows to
    /// the account, negative means the account pays V into the pool.
    net_v_to_account: i128,
    /// The whole trading fee, in vault shares.
    fee_shares: i128,
    /// The treasury's cut of `fee_shares`; the LPs keep the difference.
    reserve_fee_shares: i128,
}

impl Pricing {
    /// Reads the market and prices the curve for it. `check` runs against the
    /// market before the vault is read, for the liquidity checks that should
    /// fail cheaply and take precedence over a pricing error.
    fn load(
        e: &Env,
        check: impl FnOnce(&MarketState) -> Result<(), AmmError>,
    ) -> Result<Self, AmmError> {
        let market = get_market_state(e);
        let now = e.ledger().timestamp();
        if now >= market.expiry_ts {
            return Err(AmmError::MarketExpired);
        }
        check(&market)?;

        let secs = market.expiry_ts - now;
        let years = crate::math::seconds_to_years(secs);
        // The last few seconds before expiry round to zero years, which would
        // divide the scalar by zero: the market is closed then too.
        if years <= 0 {
            return Err(AmmError::MarketExpired);
        }

        // One vault read for the whole invocation; see VaultRate.
        let rate = VaultRate::load(e)?;
        let reserve_b_assets = rate.to_assets(market.reserve_b)?;

        let rate_scalar = crate::math::div_down(market.scalar_root, years);
        let fee_factor =
            crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, secs as i128)?;
        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            secs as i128,
        )?;

        Ok(Pricing {
            market,
            rate,
            reserve_b_assets,
            years,
            rate_scalar,
            rate_anchor,
            fee_factor,
            reserve_fee_rate: get_reserve_fee_rate(e),
        })
    }

    /// Prices `net_pt_to_account` PT through the curve: positive means PT
    /// leaves the pool to the account, negative means PT comes into it.
    fn quote(&self, net_pt_to_account: i128) -> Result<Quote, AmmError> {
        let (net_v_to_account, fee_assets, net_v_to_reserve) = calc_trade(
            self.market.reserve_a,
            self.reserve_b_assets,
            self.rate_scalar,
            self.rate_anchor,
            self.fee_factor,
            self.reserve_fee_rate,
            net_pt_to_account,
        )?;
        Ok(Quote {
            net_v_to_account,
            fee_shares: fee_in_shares(&self.rate, fee_assets)?,
            reserve_fee_shares: fee_in_shares(&self.rate, net_v_to_reserve)?,
        })
    }

    /// Closes a trade once its transfers are done: remits the treasury cut,
    /// moves the reserves by the priced amounts (`v_delta` already net of
    /// that cut, so it never enters LP accounting), re-anchors the implied
    /// rate on the new reserves and stores the market. Returns the stored
    /// market for the event.
    ///
    /// The reserves move by the priced amounts, never by observed balance
    /// changes, so donated tokens never enter pricing.
    fn settle(
        mut self,
        e: &Env,
        pt_delta: i128,
        v_delta: i128,
        reserve_fee_shares: i128,
    ) -> Result<MarketState, AmmError> {
        remit_reserve_fee(e, &self.market.token_b, reserve_fee_shares);

        self.market.reserve_a = self
            .market
            .reserve_a
            .checked_add(pt_delta)
            .ok_or(AmmError::MathOverflow)?;
        self.market.reserve_b = self
            .market
            .reserve_b
            .checked_add(v_delta)
            .ok_or(AmmError::MathOverflow)?;
        if self.market.reserve_a <= 0 || self.market.reserve_b <= 0 {
            return Err(AmmError::InvalidPoolState);
        }

        let new_exchange_rate = get_exchange_rate_from_trade(
            self.market.reserve_a,
            self.rate.to_assets(self.market.reserve_b)?,
            self.rate_scalar,
            self.rate_anchor,
            0,
        )?;
        self.market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(new_exchange_rate, self.years)?;

        put_market_state(e, &self.market);
        Ok(self.market)
    }
}

#[contractimpl]
impl AmmInterface for LiquidityPool {
    /// Sell vault shares to receive an exact amount of PT from the pool.
    ///
    /// # Arguments
    /// * `to`       - Swapper address (must authorize)
    /// * `pt_out`   - Exact amount of PT to receive from the pool
    /// * `v_in_max` - Maximum vault shares willing to pay (slippage protection)
    fn swap_v_for_pt(e: Env, to: Address, pt_out: i128, v_in_max: i128) -> Result<(), AmmError> {
        to.require_auth();
        extend_instance_ttl(&e);
        if pt_out <= 0 || v_in_max <= 0 {
            return Err(AmmError::InvalidAmount);
        }

        let pricing = Pricing::load(&e, |market| {
            if market.reserve_a <= pt_out {
                return Err(AmmError::InsufficientPtLiquidity);
            }
            Ok(())
        })?;

        let quote = pricing.quote(pt_out)?;
        // net_v_to_account is in asset units; convert to shares for the actual transfer.
        if quote.net_v_to_account >= 0 {
            return Err(AmmError::TradeTooSmall);
        }
        let v_in_assets = quote
            .net_v_to_account
            .checked_neg()
            .ok_or(AmmError::MathOverflow)?;
        let v_in_shares = pricing.rate.to_shares(v_in_assets)?;
        if v_in_shares > v_in_max {
            return Err(AmmError::MaxVInExceeded);
        }

        // Pull exactly the caller's bound and refund the unpriced remainder.
        // `v_in_shares` is computed from pool state, so it drifts between a
        // wallet's simulation and on-chain execution — it must never be the
        // amount a user's signed transfer covers. `v_in_max` is caller-chosen
        // and therefore signable; the pool's net intake is still `v_in_shares`.
        transfer_v_from_user_to_pool(&e, &pricing.market.token_b, &to, v_in_max);
        let refund = v_in_max - v_in_shares;
        if refund > 0 {
            transfer_v_from_pool_to_user(&e, &pricing.market.token_b, &to, refund);
        }
        transfer_pt_from_pool_to_user(&e, &pricing.market.token_a, &to, pt_out);

        // The treasury cut has already left the pool by the time the reserves
        // move, so it never enters them.
        let Quote { fee_shares, reserve_fee_shares, .. } = quote;
        let market = pricing.settle(&e, -pt_out, v_in_shares - reserve_fee_shares, reserve_fee_shares)?;

        SwapVForPt {
            to,
            v_in: v_in_shares,
            pt_out,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
            fee: fee_shares,
            reserve_fee: reserve_fee_shares,
        }
        .publish(&e);
        Ok(())
    }

    /// Sell an exact amount of PT into the pool and receive vault shares.
    ///
    /// # Arguments
    /// * `to`        - Swapper address (must authorize)
    /// * `pt_in`     - Exact amount of PT to sell into the pool
    /// * `min_v_out` - Minimum vault shares to receive (slippage protection)
    fn swap_pt_for_v(e: Env, to: Address, pt_in: i128, min_v_out: i128) -> Result<(), AmmError> {
        to.require_auth();
        extend_instance_ttl(&e);
        if pt_in <= 0 || min_v_out <= 0 {
            return Err(AmmError::InvalidAmount);
        }

        let pricing = Pricing::load(&e, |_| Ok(()))?;

        // Pendle sign convention: negative means PT comes FROM the user INTO the pool
        let quote = pricing.quote(-pt_in)?;
        if quote.net_v_to_account <= 0 {
            return Err(AmmError::TradeTooSmall);
        }

        // net_v_to_account is in asset units; convert to shares for transfer and slippage check.
        let v_out_shares = pricing.rate.to_shares(quote.net_v_to_account)?;
        if v_out_shares < min_v_out {
            return Err(AmmError::MinVOutNotMet);
        }
        // The fee is withheld from the user's payout but its treasury cut still
        // leaves the pool, so liquidity must cover both legs.
        let v_leaving = v_out_shares
            .checked_add(quote.reserve_fee_shares)
            .ok_or(AmmError::MathOverflow)?;
        if pricing.market.reserve_b <= v_leaving {
            return Err(AmmError::InsufficientVLiquidity);
        }

        transfer_pt_from_user_to_pool(&e, &pricing.market.token_a, &to, pt_in);
        transfer_v_from_pool_to_user(&e, &pricing.market.token_b, &to, v_out_shares);

        let Quote { fee_shares, reserve_fee_shares, .. } = quote;
        let market = pricing.settle(&e, pt_in, -v_leaving, reserve_fee_shares)?;

        SwapPtForV {
            to,
            pt_in,
            v_out: v_out_shares,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
            fee: fee_shares,
            reserve_fee: reserve_fee_shares,
        }
        .publish(&e);
        Ok(())
    }

    /// Flash side of buying YT: the pool BUYS `yt_out` PT and pays V for it.
    ///
    /// Mirror image of `flash_swap_v`. The pool prices `yt_out` PT through the curve exactly
    /// as `swap_pt_for_v` would (PT flowing in), advances that much V to the receiver, and then
    /// calls back. The receiver mints `yt_out` (PT + YT) from the advanced V plus the user's
    /// top-up, forwards the YT to the user, and delivers `yt_out` PT to this address. The pool
    /// must end the call with exactly `yt_out` more PT and `v_paid` less V, or it reverts.
    fn flash_swap_pt(
        e: Env,
        receiver: Address,
        yt_out: i128,
        user: Address,
        max_v_in: i128,
    ) -> Result<(), AmmError> {
        extend_instance_ttl(&e);
        if receiver != get_ym(&e) {
            return Err(AmmError::UntrustedReceiver);
        }
        if yt_out <= 0 || max_v_in <= 0 {
            return Err(AmmError::InvalidAmount);
        }

        let pricing = Pricing::load(&e, |_| Ok(()))?;

        // The pool buys `yt_out` PT → PT flows INTO the pool: same pricing as swap_pt_for_v.
        let quote = pricing.quote(-yt_out)?;
        if quote.net_v_to_account <= 0 {
            return Err(AmmError::TradeTooSmall);
        }
        let v_paid = pricing.rate.to_shares(quote.net_v_to_account)?;
        if v_paid <= 0 {
            return Err(AmmError::TradeTooSmall);
        }
        // Backstop only: exchange_rate >= 1 caps v_paid at yt_out, so exceeding the
        // V reserve would need proportion > 1 — calc_trade's proportion cap fires first.
        let v_leaving = v_paid
            .checked_add(quote.reserve_fee_shares)
            .ok_or(AmmError::MathOverflow)?;
        if pricing.market.reserve_b <= v_leaving {
            return Err(AmmError::InsufficientVLiquidity);
        }

        let pt_balance_before = get_balance_a(&e);
        let v_balance_before = get_balance_b(&e);

        // Advance V to the receiver — pool is temporarily short V here.
        token::TokenClient::new(&e, &pricing.market.token_b)
            .transfer(&e.current_contract_address(), &receiver, &v_paid);

        // Synchronous callback: receiver mints yt_out (PT+YT), sends YT to the user,
        // and delivers exactly yt_out PT back to this address.
        // `rate` is passed rather than left for the receiver to fetch: it was read
        // from the vault a few lines above and cannot move within this transaction.
        FlashSwapPtReceiverClient::new(&e, &receiver).on_flash_receive_pt(
            &yt_out,
            &v_paid,
            &user,
            &max_v_in,
            &pricing.rate.assets_per_scale(),
            &e.current_contract_address(),
        );

        // Invariant: pool gained exactly yt_out PT and paid exactly v_paid V.
        // Checked before settle remits the fee, so the deltas see only the
        // priced amounts.
        let pt_balance_after = get_balance_a(&e);
        let v_balance_after = get_balance_b(&e);
        if pt_balance_after != pt_balance_before + yt_out
            || v_balance_after != v_balance_before - v_paid
        {
            return Err(AmmError::FlashSwapNotSettled);
        }

        let Quote { fee_shares, reserve_fee_shares, .. } = quote;
        let market = pricing.settle(&e, yt_out, -v_leaving, reserve_fee_shares)?;

        FlashSwapPt {
            receiver,
            user,
            pt_bought: yt_out,
            v_paid,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
            fee: fee_shares,
            reserve_fee: reserve_fee_shares,
        }
        .publish(&e);
        Ok(())
    }

    /// Flash-lends PT to a receiver and is repaid in vault shares (V).
    ///
    /// Mirror image of `flash_swap_pt`: from the pool's perspective this is a `swap_v_for_pt`
    /// trade — PT leaves the pool, V comes in — except the PT recipient is the receiver's
    /// callback (which combines it with the user's YT and redeems both for V via the yield
    /// manager). The pool prices the lent PT through the same curve and requires that exact
    /// amount of V back before the callback returns. The lent PT does not return (it is burned
    /// in the redeem), so `reserve_a` falls by `pt_to_borrow`.
    fn flash_swap_v(
        e: Env,
        receiver: Address,
        pt_to_borrow: i128,
        user: Address,
        min_v_out: i128,
    ) -> Result<(), AmmError> {
        extend_instance_ttl(&e);
        if receiver != get_ym(&e) {
            return Err(AmmError::UntrustedReceiver);
        }
        if pt_to_borrow <= 0 || min_v_out <= 0 {
            return Err(AmmError::InvalidAmount);
        }

        let pricing = Pricing::load(&e, |market| {
            if market.reserve_a <= pt_to_borrow {
                return Err(AmmError::InsufficientPtLiquidity);
            }
            Ok(())
        })?;

        // PT flows OUT of the pool to the account → positive net_pt_to_account, V owed back.
        let quote = pricing.quote(pt_to_borrow)?;
        if quote.net_v_to_account >= 0 {
            return Err(AmmError::TradeTooSmall);
        }
        let v_owed_assets = quote
            .net_v_to_account
            .checked_neg()
            .ok_or(AmmError::MathOverflow)?;
        let v_owed_shares = pricing.rate.to_shares(v_owed_assets)?;
        if v_owed_shares <= 0 {
            return Err(AmmError::TradeTooSmall);
        }

        let pt_balance_before = get_balance_a(&e);
        let v_balance_before = get_balance_b(&e);

        // Lend PT — pool is temporarily under-collateralized here.
        token::TokenClient::new(&e, &pricing.market.token_a)
            .transfer(&e.current_contract_address(), &receiver, &pt_to_borrow);

        // Synchronous callback: receiver pulls YT from the user, redeems PT+YT → V via the YM,
        // repays this address `v_owed_shares` V, and forwards the remainder to the user.
        // Same as flash_swap_pt: hand down the rate already loaded above.
        FlashSwapVReceiverClient::new(&e, &receiver).on_flash_receive_v(
            &pt_to_borrow,
            &v_owed_shares,
            &user,
            &min_v_out,
            &pricing.rate.assets_per_scale(),
            &e.current_contract_address(),
        );

        // Invariant: the lent PT was consumed by the redeem and V was fully repaid.
        // Checked before settle remits the fee, so the repayment check sees only
        // the priced amounts.
        let pt_balance_after = get_balance_a(&e);
        let v_balance_after = get_balance_b(&e);
        if pt_balance_after != pt_balance_before - pt_to_borrow
            || v_balance_after < v_balance_before + v_owed_shares
        {
            return Err(AmmError::FlashSwapNotSettled);
        }

        // PT fell (lent then burned); V rose by the owed repayment, which
        // includes the fee, net of the treasury cut. Any overpayment stays
        // out of pricing.
        let Quote { fee_shares, reserve_fee_shares, .. } = quote;
        let market = pricing.settle(&e, -pt_to_borrow, v_owed_shares - reserve_fee_shares, reserve_fee_shares)?;

        FlashSwapV {
            receiver,
            user,
            pt_borrowed: pt_to_borrow,
            v_owed: v_owed_shares,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
            fee: fee_shares,
            reserve_fee: reserve_fee_shares,
        }
        .publish(&e);
        Ok(())
    }

    /// Deposits tokens into the pool and mints shares. Deposit ratio must match
    /// the current pool ratio (any ratio accepted if pool is empty).
    ///
    /// # Arguments
    /// * `to` - Depositor address (must authorize)
    /// * `desired_a` - Desired amount of token A (PT)
    /// * `min_a` - Minimum acceptable amount of token A
    /// * `desired_b` - Desired amount of token B (vault shares)
    /// * `min_b` - Minimum acceptable amount of token B
    fn deposit(
        e: Env,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) -> Result<(), AmmError> {
        to.require_auth();
        extend_instance_ttl(&e);

        let mut market = get_market_state(&e);

        let now = e.ledger().timestamp();
        if now >= market.expiry_ts {
            return Err(AmmError::MarketExpired);
        }

        let (amount_a, amount_b) =
            get_deposit_amounts(desired_a, min_a, desired_b, min_b, market.reserve_a, market.reserve_b)?;

        if amount_a <= 0 || amount_b <= 0 {
            return Err(AmmError::DepositTooSmall);
        }

        let token_a_client = token::TokenClient::new(&e, &market.token_a);
        let token_b_client = token::TokenClient::new(&e, &market.token_b);

        // Pull exactly the caller-chosen desired amounts and refund what the
        // ratio doesn't take. The ratio-derived (amount_a, amount_b) drift with
        // pool state between a wallet's simulation and execution, so they must
        // never be the amounts a user's signed transfers cover; desired_a/b are
        // the caller's own numbers. Refunds happen before the balance reads
        // below, so share pricing sees only what the pool kept.
        token_a_client.transfer(&to, &e.current_contract_address(), &desired_a);
        token_b_client.transfer(&to, &e.current_contract_address(), &desired_b);
        if desired_a > amount_a {
            token_a_client.transfer(&e.current_contract_address(), &to, &(desired_a - amount_a));
        }
        if desired_b > amount_b {
            token_b_client.transfer(&e.current_contract_address(), &to, &(desired_b - amount_b));
        }

        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        let zero = 0;
        let new_total_shares = if total_shares == zero {
            (amount_a * amount_b).sqrt()
        } else if market.reserve_a > zero && market.reserve_b > zero {
            let shares_a = (balance_a * total_shares) / market.reserve_a;
            let shares_b = (balance_b * total_shares) / market.reserve_b;
            shares_a.min(shares_b)
        } else {
            // Reserves are empty but shares exist.
            return Err(AmmError::InvalidPoolState);
        };

        let shares_to_mint = new_total_shares - total_shares;
        if total_shares == zero {
            // First deposit: `sqrt(a*b)` must exceed the dead-burn, otherwise the
            // subtraction below underflows (or mints the depositor zero/negative
            // shares) — a silent loss of the entire initial deposit.
            if shares_to_mint <= MINIMUM_LIQUIDITY {
                return Err(AmmError::DepositTooSmall);
            }
            let burn_address = Address::from_str(&e, BURN_ADDRESS);
            mint_shares(&e, &burn_address, MINIMUM_LIQUIDITY);
            mint_shares(&e, &to, shares_to_mint - MINIMUM_LIQUIDITY);
        } else {
            // Floor division can round the minted amount all the way to zero when
            // the pool holds few shares against large reserves (e.g. after heavy
            // one-sided swap volume). Reject rather than take the tokens for free.
            if shares_to_mint <= 0 {
                return Err(AmmError::DepositTooSmall);
            }
            mint_shares(&e, &to, shares_to_mint);
        }

        market.reserve_a = balance_a;
        market.reserve_b = balance_b;
        put_market_state(&e, &market);

        Deposit {
            to,
            amount_a,
            amount_b,
            shares_minted: shares_to_mint,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
        }
        .publish(&e);
        Ok(())
    }

    /// Burns pool shares and withdraws a proportional amount of both tokens.
    ///
    /// # Arguments
    /// * `to`           - Withdrawer address (must authorize and own the shares)
    /// * `share_amount` - Number of pool shares to burn
    /// * `min_a`        - Minimum acceptable amount of token A (PT)
    /// * `min_b`        - Minimum acceptable amount of token B (vault shares)
    ///
    /// # Returns
    /// `(amount_a, amount_b)` actually withdrawn
    fn withdraw(
        e: Env,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> Result<(i128, i128), AmmError> {
        to.require_auth();
        extend_instance_ttl(&e);
        // A non-positive share_amount passes the balance check below (0 < -n is
        // false) and reaches burn_shares, where subtracting a negative MINTS LP
        // shares. Reject it here rather than rely on a token leg refusing the
        // resulting zero/negative transfer — PT does not, and which vault-share
        // token does depends on the market.
        if share_amount <= 0 || min_a < 0 || min_b < 0 {
            return Err(AmmError::InvalidAmount);
        }

        let current_shares = get_shares(&e, &to);
        if current_shares < share_amount {
            return Err(AmmError::InsufficientShares);
        }

        let mut market = get_market_state(&e);
        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        let out_a = (balance_a * share_amount) / total_shares;
        let out_b = (balance_b * share_amount) / total_shares;

        if out_a < min_a || out_b < min_b {
            return Err(AmmError::WithdrawMinNotMet);
        }

        burn_shares(&e, &to, share_amount);
        transfer_pt_from_pool_to_user(&e, &market.token_a, &to, out_a);
        transfer_v_from_pool_to_user(&e, &market.token_b, &to, out_b);

        market.reserve_a = balance_a - out_a;
        market.reserve_b = balance_b - out_b;
        put_market_state(&e, &market);

        Withdraw {
            to,
            share_amount,
            amount_a: out_a,
            amount_b: out_b,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
        }
        .publish(&e);

        Ok((out_a, out_b))
    }

    /// Returns the current reserves of both tokens.
    ///
    /// # Returns
    /// `(reserve_pt, reserve_v)` — PT reserve and vault share reserve
    fn get_reserves(e: Env) -> (i128, i128) {
        extend_instance_ttl(&e);
        let market = get_market_state(&e);
        (market.reserve_a, market.reserve_b)
    }

    fn get_implied_rate(e: Env) -> i128 {
        extend_instance_ttl(&e);
        get_market_state(&e).last_implied_rate
    }

    fn get_treasury(e: Env) -> Address {
        extend_instance_ttl(&e);
        get_treasury(&e)
    }

    fn get_reserve_fee_rate(e: Env) -> i128 {
        extend_instance_ttl(&e);
        get_reserve_fee_rate(&e)
    }

    /// Returns the pool share balance for a given user.
    fn balance_shares(e: Env, user: Address) -> i128 {
        extend_instance_ttl(&e);
        get_shares(&e, &user)
    }

    /// Returns the total pool shares outstanding (including the locked
    /// minimum-liquidity shares held by the burn address).
    fn get_total_shares(e: Env) -> i128 {
        extend_instance_ttl(&e);
        get_total_shares(&e)
    }
}