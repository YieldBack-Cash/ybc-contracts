use crate::curve::{calc_trade, compute_rate_anchor, get_exchange_rate_from_trade};
use crate::events::{Deposit, FlashSwapPt, FlashSwapV, PoolInit, SwapPtForV, SwapVForPt, Withdraw};
use crate::transfers::{get_deposit_amounts, transfer_pt_from_pool_to_user, transfer_pt_from_user_to_pool, transfer_v_from_user_to_pool, transfer_v_from_pool_to_user};
use crate::vault::{convert_assets_to_vault_shares, convert_vault_shares_to_assets};
use crate::storage::*;
use num_integer::Roots;
use amm_interface::{AmmInterface, FlashSwapPtReceiverClient, FlashSwapVReceiverClient};
use soroban_sdk::{contract, contractimpl, token, Address, Env};

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
    ) {
        let now = e.ledger().timestamp();
        assert!(expiry_ts > now, "expiry must be in the future");
        assert!(apy_min >= 0, "apy_min must be non-negative");
        assert!(
            apy_min < current_apy && current_apy < apy_max,
            "current_apy must be inside the band"
        );
        assert!(apy_max <= MAX_APY, "apy_max too high");
        assert!(apy_max - apy_min >= MIN_BAND_WIDTH, "band too narrow");
        assert!(fee_apy > 0 && fee_apy <= MAX_FEE_APY, "fee_apy out of range");

        // TODO: verify this derivation — double-check that pinning the band
        // edges at p = 0.9 / 0.1 PT proportion (±ln(9) logit term) actually
        // produces the intended apy_min/apy_max behavior across the curve,
        // including away-from-first-order cases (time close to expiry, wide
        // bands, etc). Sanity-check against the curve math in curve.rs.
        //
        // The curve stores rates in ln space (exchange_rate = e^(rate·t)), so
        // an APY maps to ln(1 + apy). The band collapses into curve steepness:
        // at the p = 0.9 / 0.1 pins the logit term is ±ln(9), and to first
        // order the resulting APY half-width ln(9)/scalar_root is the same at
        // any time to expiry.
        let last_implied_rate =
            crate::math::ln_fp(crate::math::FP_SCALE + current_apy, crate::math::FP_SCALE);
        let fee_rate_root =
            crate::math::ln_fp(crate::math::FP_SCALE + fee_apy, crate::math::FP_SCALE);
        let scalar_root = crate::math::div_down(2 * LN_9, apy_max - apy_min);

        set_ym(&e, &ym);

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
        }
        .publish(&e);
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
    fn swap_v_for_pt(e: Env, to: Address, pt_out: i128, v_in_max: i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(pt_out > 0, "pt_out must be positive");
        assert!(v_in_max > 0, "v_in_max must be positive");

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts, "market expired");
        assert!(market.reserve_a > pt_out, "insufficient PT liquidity");

        let time_to_expiry = market.expiry_ts - now;
        let years = crate::math::seconds_to_years(time_to_expiry);

        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);

        let rate_scalar = crate::math::div_down(market.scalar_root, years);
        let fee_factor = crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, time_to_expiry as i128);
        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            time_to_expiry as i128,
        );

        let (net_v_to_account, _fee, _reserve_fee) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0, // reserve fee off for now
            pt_out,
        );

        // net_v_to_account is in asset units; convert to shares for the actual transfer.
        assert!(net_v_to_account < 0, "expected user to pay V in");
        let v_in_assets = net_v_to_account
            .checked_neg()
            .expect("overflow converting signed V flow");
        let v_in_shares = convert_assets_to_vault_shares(&e, v_in_assets);
        assert!(v_in_shares <= v_in_max, "in amount is over max");

        transfer_v_from_user_to_pool(&e, &market.token_b, &to, v_in_shares);
        transfer_pt_from_pool_to_user(&e, &market.token_a, &to, pt_out);

        // reserve_b stays in vault shares for LP accounting.
        market.reserve_b += v_in_shares;
        market.reserve_a -= pt_out;

        let new_exchange_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0,
        );

        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(new_exchange_rate, years);

        put_market_state(&e, &market);

        SwapVForPt {
            to,
            v_in: v_in_shares,
            pt_out,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
        }
        .publish(&e);
    }

    /// Sell an exact amount of PT into the pool and receive vault shares.
    ///
    /// # Arguments
    /// * `to`        - Swapper address (must authorize)
    /// * `pt_in`     - Exact amount of PT to sell into the pool
    /// * `min_v_out` - Minimum vault shares to receive (slippage protection)
    fn swap_pt_for_v(e: Env, to: Address, pt_in: i128, min_v_out: i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(pt_in > 0, "pt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts, "market expired");

        let t_secs = (market.expiry_ts - now) as i128;
        assert!(t_secs > 0, "time to expiry must be positive");

        let t_years = crate::math::seconds_to_years(market.expiry_ts - now);
        assert!(t_years > 0, "time to expiry in years must be positive");

        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);

        let rate_scalar = crate::math::div_down(market.scalar_root, t_years);
        let fee_factor = crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, t_secs);

        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            t_secs,
        );

        // Pendle sign convention: negative means PT comes FROM the user INTO the pool
        let net_pt_to_account = -pt_in;

        let (net_v_to_account, _net_v_fee, _net_v_to_reserve) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0,
            net_pt_to_account,
        );

        assert!(net_v_to_account > 0, "expected positive V flow to account for PT-in trade");

        // net_v_to_account is in asset units; convert to shares for transfer and slippage check.
        let v_out_assets = net_v_to_account;
        let v_out_shares = convert_assets_to_vault_shares(&e, v_out_assets);
        assert!(v_out_shares >= min_v_out, "out amount below minimum");
        assert!(market.reserve_b > v_out_shares, "insufficient V liquidity");

        let new_reserve_a = market.reserve_a
            .checked_add(pt_in)
            .expect("overflow updating PT reserve");
        // reserve_b stays in vault shares for LP accounting.
        let new_reserve_b = market.reserve_b
            .checked_sub(v_out_shares)
            .expect("underflow updating V reserve");

        assert!(new_reserve_a > 0 && new_reserve_b > 0, "new reserves must be strictly positive");

        transfer_pt_from_user_to_pool(&e, &market.token_a, &to, pt_in);
        transfer_v_from_pool_to_user(&e, &market.token_b, &to, v_out_shares);

        market.reserve_a = new_reserve_a;
        market.reserve_b = new_reserve_b;

        let ex_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0,
        );

        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(ex_rate, t_years);

        put_market_state(&e, &market);

        SwapPtForV {
            to,
            pt_in,
            v_out: v_out_shares,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
        }
        .publish(&e);
    }

    /// Flash side of buying YT: the pool BUYS `yt_out` PT and pays V for it.
    ///
    /// Mirror image of `flash_swap_v`. The pool prices `yt_out` PT through the curve exactly
    /// as `swap_pt_for_v` would (PT flowing in), advances that much V to the receiver, and then
    /// calls back. The receiver mints `yt_out` (PT + YT) from the advanced V plus the user's
    /// top-up, forwards the YT to the user, and delivers `yt_out` PT to this address. The pool
    /// must end the call with exactly `yt_out` more PT and `v_paid` less V, or it reverts.
    fn flash_swap_pt(e: Env, receiver: Address, yt_out: i128, user: Address, max_v_in: i128) {
        extend_instance_ttl(&e);
        assert_eq!(receiver, get_ym(&e), "receiver must be the trusted yield manager");
        assert!(yt_out > 0, "yt_out must be positive");
        assert!(max_v_in > 0, "max_v_in must be positive");

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts, "market expired");

        let t_secs = (market.expiry_ts - now) as i128;
        let t_years = crate::math::seconds_to_years(market.expiry_ts - now);
        assert!(t_years > 0, "time to expiry in years must be positive");

        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);
        let rate_scalar = crate::math::div_down(market.scalar_root, t_years);
        let fee_factor = crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, t_secs);
        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            t_secs,
        );

        // The pool buys `yt_out` PT → PT flows INTO the pool: same pricing as swap_pt_for_v.
        let net_pt_to_account = -yt_out;
        let (net_v_to_account, _fee, _reserve_fee) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0, // reserve fee off for now
            net_pt_to_account,
        );
        assert!(net_v_to_account > 0, "expected pool to pay V for PT");
        let v_paid = convert_assets_to_vault_shares(&e, net_v_to_account);
        assert!(v_paid > 0, "v_paid must be positive");
        // Backstop only: exchange_rate >= 1 caps v_paid at yt_out, so exceeding the
        // V reserve would need proportion > 1 — calc_trade's proportion cap fires first.
        assert!(market.reserve_b > v_paid, "insufficient V liquidity");

        let pt_balance_before = get_balance_a(&e);
        let v_balance_before = get_balance_b(&e);

        // Advance V to the receiver — pool is temporarily short V here.
        token::TokenClient::new(&e, &market.token_b)
            .transfer(&e.current_contract_address(), &receiver, &v_paid);

        // Synchronous callback: receiver mints yt_out (PT+YT), sends YT to the user,
        // and delivers exactly yt_out PT back to this address.
        FlashSwapPtReceiverClient::new(&e, &receiver)
            .on_flash_receive_pt(&yt_out, &v_paid, &user, &max_v_in, &e.current_contract_address());

        // Invariant: pool gained exactly yt_out PT and paid exactly v_paid V.
        let pt_balance_after = get_balance_a(&e);
        let v_balance_after = get_balance_b(&e);
        assert_eq!(pt_balance_after, pt_balance_before + yt_out, "flash swap: PT not delivered");
        assert_eq!(v_balance_after, v_balance_before - v_paid, "flash swap: V mispaid");

        // Update reserves by the priced amounts; the balance checks above are
        // assertions only, so donated tokens never enter pricing.
        market.reserve_a = market
            .reserve_a
            .checked_add(yt_out)
            .expect("overflow updating PT reserve");
        market.reserve_b = market
            .reserve_b
            .checked_sub(v_paid)
            .expect("underflow updating V reserve");
        assert!(market.reserve_a > 0 && market.reserve_b > 0, "new reserves must be strictly positive");

        let new_exchange_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0,
        );
        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(new_exchange_rate, t_years);

        put_market_state(&e, &market);

        FlashSwapPt {
            receiver,
            user,
            pt_bought: yt_out,
            v_paid,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
        }
        .publish(&e);
    }

    /// Flash-lends PT to a receiver and is repaid in vault shares (V).
    ///
    /// Mirror image of `flash_swap_pt`: from the pool's perspective this is a `swap_v_for_pt`
    /// trade — PT leaves the pool, V comes in — except the PT recipient is the receiver's
    /// callback (which combines it with the user's YT and redeems both for V via the yield
    /// manager). The pool prices the lent PT through the same curve and requires that exact
    /// amount of V back before the callback returns. The lent PT does not return (it is burned
    /// in the redeem), so `reserve_a` falls by `pt_to_borrow`.
    fn flash_swap_v(e: Env, receiver: Address, pt_to_borrow: i128, user: Address, min_v_out: i128) {
        extend_instance_ttl(&e);
        assert_eq!(receiver, get_ym(&e), "receiver must be the trusted yield manager");
        assert!(pt_to_borrow > 0, "pt_to_borrow must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts, "market expired");
        assert!(market.reserve_a > pt_to_borrow, "insufficient PT liquidity");

        let time_to_expiry = market.expiry_ts - now;
        let years = crate::math::seconds_to_years(time_to_expiry);

        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);

        let rate_scalar = crate::math::div_down(market.scalar_root, years);
        let fee_factor = crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, time_to_expiry as i128);
        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            time_to_expiry as i128,
        );

        // PT flows OUT of the pool to the account → positive net_pt_to_account, V owed back.
        let (net_v_to_account, _fee, _reserve_fee) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0, // reserve fee off for now
            pt_to_borrow,
        );
        assert!(net_v_to_account < 0, "expected pool to be repaid V");
        let v_owed_assets = net_v_to_account
            .checked_neg()
            .expect("overflow converting signed V flow");
        let v_owed_shares = convert_assets_to_vault_shares(&e, v_owed_assets);
        assert!(v_owed_shares > 0, "v_owed must be positive");

        let pt_balance_before = get_balance_a(&e);
        let v_balance_before = get_balance_b(&e);

        // Lend PT — pool is temporarily under-collateralized here.
        token::TokenClient::new(&e, &market.token_a)
            .transfer(&e.current_contract_address(), &receiver, &pt_to_borrow);

        // Synchronous callback: receiver pulls YT from the user, redeems PT+YT → V via the YM,
        // repays this address `v_owed_shares` V, and forwards the remainder to the user.
        FlashSwapVReceiverClient::new(&e, &receiver)
            .on_flash_receive_v(&pt_to_borrow, &v_owed_shares, &user, &min_v_out, &e.current_contract_address());

        let pt_balance_after = get_balance_a(&e);
        let v_balance_after = get_balance_b(&e);
        assert_eq!(pt_balance_after, pt_balance_before - pt_to_borrow, "flash swap: lent PT must be consumed by the redeem");
        assert!(
            v_balance_after >= v_balance_before + v_owed_shares,
            "flash swap: V not fully repaid"
        );

        // Update reserves by the priced amounts: PT fell (lent then burned), V rose
        // by the owed repayment. Any overpayment stays out of pricing.
        market.reserve_a = market
            .reserve_a
            .checked_sub(pt_to_borrow)
            .expect("underflow updating PT reserve");
        market.reserve_b = market
            .reserve_b
            .checked_add(v_owed_shares)
            .expect("overflow updating V reserve");
        assert!(market.reserve_a > 0 && market.reserve_b > 0, "new reserves must be strictly positive");

        let new_exchange_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0,
        );
        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(new_exchange_rate, years);

        put_market_state(&e, &market);

        FlashSwapV {
            receiver,
            user,
            pt_borrowed: pt_to_borrow,
            v_owed: v_owed_shares,
            new_implied_rate: market.last_implied_rate,
            new_reserve_a: market.reserve_a,
            new_reserve_b: market.reserve_b,
        }
        .publish(&e);
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
    ) {
        to.require_auth();
        extend_instance_ttl(&e);

        let mut market = get_market_state(&e);

        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts, "market expired");

        let (amount_a, amount_b) =
            get_deposit_amounts(desired_a, min_a, desired_b, min_b, market.reserve_a, market.reserve_b);

        if amount_a <= 0 || amount_b <= 0 {
            panic!("both amounts must be strictly positive");
        }

        let token_a_client = token::TokenClient::new(&e, &market.token_a);
        let token_b_client = token::TokenClient::new(&e, &market.token_b);

        token_a_client.transfer(&to, &e.current_contract_address(), &amount_a);
        token_b_client.transfer(&to, &e.current_contract_address(), &amount_b);

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
            panic!("reserves are empty but shares exist");
        };

        let shares_to_mint = new_total_shares - total_shares;
        if total_shares == zero {
            // First deposit: `sqrt(a*b)` must exceed the dead-burn, otherwise the
            // subtraction below underflows (or mints the depositor zero/negative
            // shares) — a silent loss of the entire initial deposit.
            assert!(
                shares_to_mint > MINIMUM_LIQUIDITY,
                "initial deposit too small: would mint zero shares after minimum liquidity burn"
            );
            let burn_address = Address::from_str(&e, BURN_ADDRESS);
            mint_shares(&e, &burn_address, MINIMUM_LIQUIDITY);
            mint_shares(&e, &to, shares_to_mint - MINIMUM_LIQUIDITY);
        } else {
            // Floor division can round the minted amount all the way to zero when
            // the pool holds few shares against large reserves (e.g. after heavy
            // one-sided swap volume). Reject rather than take the tokens for free.
            assert!(shares_to_mint > 0, "deposit too small: would mint zero shares");
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
    ) -> (i128, i128) {
        to.require_auth();
        extend_instance_ttl(&e);

        let current_shares = get_shares(&e, &to);
        if current_shares < share_amount {
            panic!("insufficient shares");
        }

        let mut market = get_market_state(&e);
        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        let out_a = (balance_a * share_amount) / total_shares;
        let out_b = (balance_b * share_amount) / total_shares;

        if out_a < min_a || out_b < min_b {
            panic!("min not satisfied");
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

        (out_a, out_b)
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