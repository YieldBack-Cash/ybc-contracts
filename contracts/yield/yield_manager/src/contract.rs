use crate::events::{
    Deposit, DepositAsset, DistributeYield, FlashDeposit, FlashRedeem, PoolSet, RedeemCombined,
    RedeemPrincipal, RedeemToAsset, SurplusCollected, TokenContractsSet,
};
use crate::storage;
use amm_interface::{FlashSwapPtReceiver, FlashSwapVReceiver};
use principal_token_interface::PrincipalTokenClient;
use soroban_sdk::{token, Address, Env};
use vault_interface::VaultContractClient;
use yield_manager_interface::{YieldManagerError, YieldManagerTrait};
use yield_token_interface::YieldTokenClient;

use ybc_common::scale::SCALAR_7;

use soroban_sdk::{contract, contractimpl, panic_with_error};

#[contract]
pub struct YieldManager;

impl YieldManager {
    // ── entry-point guards ──────────────────────────────────────────────────
    //
    // Every entry point opens the same way: keep the instance alive, refuse to
    // run before `set_token_contracts`, refuse a non-positive amount, and
    // refuse the wrong side of maturity. Written once so the next rule lands
    // in one place.

    fn open(env: &Env) -> Result<(), YieldManagerError> {
        storage::extend_instance_ttl(env);
        if !storage::is_initialized(env) {
            return Err(YieldManagerError::NotInitialized);
        }
        Ok(())
    }

    fn positive(amount: i128) -> Result<(), YieldManagerError> {
        if amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        Ok(())
    }

    /// Minting and combining are pre-maturity only: after it, PT would mint
    /// at the locked rate but redeem at the live one, and combining PT+YT
    /// would burn YT for no extra shares.
    fn before_maturity(env: &Env) -> Result<(), YieldManagerError> {
        if env.ledger().timestamp() >= storage::get_maturity(env) {
            return Err(YieldManagerError::MaturityReached);
        }
        Ok(())
    }

    /// Principal redemption is post-maturity only.
    fn after_maturity(env: &Env) -> Result<(), YieldManagerError> {
        if env.ledger().timestamp() < storage::get_maturity(env) {
            return Err(YieldManagerError::MaturityNotReached);
        }
        Ok(())
    }

    // ── rates ───────────────────────────────────────────────────────────────

    /// Current vault rate, expressed as assets per SCALAR_7 shares (1e7-scaled).
    /// Every vault is read the same way, through SEP-56 `convert_to_assets`;
    /// what differs between lending protocols is the adapter's business.
    fn get_vault_exchange_rate(env: &Env) -> i128 {
        VaultContractClient::new(env, &storage::get_vault(env)).convert_to_assets(&SCALAR_7)
    }

    /// Refreshes the stored rate before maturity (rate can only increase, and locks
    /// once maturity is reached) and returns the resulting current rate.
    ///
    /// This is the protocol's single source of truth for the rate, and the AMM
    /// reads it through `get_exchange_rate` rather than probing the vault itself.
    /// It has to: PT settles at this number, so anything pricing PT against the
    /// vault's own rate misprices it the moment the two diverge. See
    /// `amm/src/vault.rs` and `tests/integration/src/tests/rate_divergence.rs`.
    fn update_exchange_rate(env: &Env) -> i128 {
        if storage::is_rate_locked(env) {
            return storage::get_exchange_rate(env);
        }

        // Checked before the vault read, so a locked market never touches the vault.
        YieldManager::update_exchange_rate_from(env, YieldManager::get_vault_exchange_rate(env))
    }

    /// As [`YieldManager::update_exchange_rate`], with the rate supplied by the
    /// caller. Only the flash callbacks use it: the pool obtained this value
    /// from `get_exchange_rate` to price the trade (which already committed
    /// the ratchet), so re-applying it here is idempotent and saves a second
    /// vault round trip. The floor and the maturity lock still apply. Safe
    /// only because the callbacks are gated to the registered pool and
    /// `set_pool` is one-shot; `create_market` wires the same `vault` into
    /// both contracts.
    fn update_exchange_rate_from(env: &Env, rate_hint: i128) -> i128 {
        if storage::is_rate_locked(env) {
            return storage::get_exchange_rate(env);
        }
        if rate_hint <= 0 {
            panic_with_error!(env, YieldManagerError::ExchangeRateZero);
        }

        let maturity = storage::get_maturity(env);
        let current_time = env.ledger().timestamp();

        let stored_rate = storage::get_exchange_rate(env);

        let current_rate = if rate_hint > stored_rate {
            storage::set_exchange_rate(env, rate_hint);
            rate_hint
        } else {
            stored_rate
        };

        if current_time >= maturity {
            storage::set_rate_locked(env);
        }

        current_rate
    }

    /// The current rate after the ratchet. The zero check is defence in depth:
    /// the constructor refuses a non-positive seed and the ratchet never lowers
    /// it, so it cannot fire.
    fn current_rate(env: &Env) -> Result<i128, YieldManagerError> {
        let rate = YieldManager::update_exchange_rate(env);
        if rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }
        Ok(rate)
    }

    /// The two rates a post-maturity settlement needs: the locked rate the
    /// position was reserved at, and the live vault rate floored at it.
    ///
    /// Positions settle at the live rate so PT always pays exactly face value
    /// in assets, while the shares that keep appreciating after maturity stay
    /// in the YM as protocol surplus. The floor at the locked rate means a
    /// vault rate dip can never pay out more shares than were reserved.
    fn settlement_rates(env: &Env) -> Result<(i128, i128), YieldManagerError> {
        let locked = YieldManager::current_rate(env)?;
        let live = YieldManager::get_vault_exchange_rate(env).max(locked);
        Ok((locked, live))
    }

    // ── the face-value rule ─────────────────────────────────────────────────
    //
    // Overflow in this arithmetic is a bug, not a user error; a host panic is
    // the right outcome, so the `expect`s below are deliberate.

    /// Vault shares for `amount` of asset value at `rate`, floored.
    fn shares_for_assets(amount: i128, rate: i128) -> i128 {
        amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares")
            / rate
    }

    /// Asset value of `shares` at `rate`, floored.
    fn assets_for_shares(shares: i128, rate: i128) -> i128 {
        shares.checked_mul(rate).expect("overflow computing assets") / SCALAR_7
    }

    /// Shares a post-maturity redemption of `pt_amount` PT pays out, with the
    /// surplus banked.
    ///
    /// The PT's backing was reserved at the locked rate; redeeming at a higher
    /// live rate frees the difference, the post-maturity interest the redeemer
    /// forgoes ("you snooze you lose"). It is recorded for `collect_surplus`.
    /// Every path that burns PT after maturity goes through here, so the
    /// face-value rule has one home.
    fn shares_for_principal(env: &Env, pt_amount: i128, locked: i128, live: i128) -> i128 {
        let shares = YieldManager::shares_for_assets(pt_amount, live);
        let shares_at_locked = YieldManager::shares_for_assets(pt_amount, locked);
        YieldManager::bank_surplus(env, shares_at_locked - shares);
        shares
    }

    /// Records shares no user has a claim on any more.
    fn bank_surplus(env: &Env, freed: i128) {
        if freed > 0 {
            storage::set_surplus_shares(env, storage::get_surplus_shares(env) + freed);
        }
    }

    // ── custody ─────────────────────────────────────────────────────────────

    /// Redeems `shares` from the YM's own custody through the vault, with the
    /// vault paying the underlying directly to `to`. The YM is owner and
    /// operator alike, so the vault's authorization is satisfied by invoker
    /// auth at execution time — nothing here ever enters a user's signature,
    /// which is what lets callers pass freshly measured share counts. Returns
    /// the asset delivered, measured as a balance delta (SEP-56 leaves fees and
    /// rounding to the vault).
    fn redeem_custody_to(env: &Env, to: &Address, shares: i128) -> i128 {
        let vault_addr = storage::get_vault(env);
        let ym = env.current_contract_address();
        let vault_client = VaultContractClient::new(env, &vault_addr);
        let asset_token = token::Client::new(env, &vault_client.query_asset());

        let before = asset_token.balance(to);
        vault_client.redeem(&shares, to, &ym, &ym);
        asset_token.balance(to) - before
    }

    /// Mints `amount` PT to `pt_to` and `amount` YT to `yt_to` at
    /// `exchange_rate`. The YM is the direct caller of both mints, satisfying
    /// each token's admin auth.
    fn mint_pair(env: &Env, pt_to: &Address, yt_to: &Address, amount: i128, exchange_rate: i128) {
        PrincipalTokenClient::new(env, &storage::get_principal_token(env)).mint(pt_to, &amount);
        YieldTokenClient::new(env, &storage::get_yield_token(env)).mint(
            yt_to,
            &amount,
            &exchange_rate,
        );
    }

    /// Burns `amount` of PT and YT from `from`. The rate hint stops the YT
    /// contract calling back into the YM for it (re-entry while the YM is on
    /// the call stack is rejected by the host). Burn amounts are caller-chosen,
    /// so the user's signed burn entries are fixed.
    fn burn_pair(env: &Env, from: &Address, amount: i128, exchange_rate: i128) {
        token::Client::new(env, &storage::get_principal_token(env)).burn(from, &amount);
        YieldTokenClient::new(env, &storage::get_yield_token(env)).burn_with_rate(
            from,
            &amount,
            &exchange_rate,
        );
    }
}

#[contractimpl]
impl YieldManagerTrait for YieldManager {
    fn __constructor(env: Env, admin: Address, vault: Address, maturity: u64, treasury: Address) {
        storage::set_admin(&env, &admin);
        storage::set_vault(&env, &vault);
        storage::set_maturity(&env, maturity);
        storage::set_treasury(&env, &treasury);

        // The only path by which a rate enters storage unratcheted, so it is the
        // one place a non-positive rate has to be refused.
        let initial_rate = YieldManager::get_vault_exchange_rate(&env);
        if initial_rate <= 0 {
            panic_with_error!(&env, YieldManagerError::ExchangeRateZero);
        }
        storage::set_exchange_rate(&env, initial_rate);
    }

    fn set_token_contracts(
        env: Env,
        pt_addr: Address,
        yt_addr: Address,
    ) -> Result<(), YieldManagerError> {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        if storage::is_initialized(&env) {
            return Err(YieldManagerError::AlreadyInitialized);
        }

        storage::set_principal_token(&env, &pt_addr);
        storage::set_yield_token(&env, &yt_addr);

        TokenContractsSet {
            pt: pt_addr,
            yt: yt_addr,
        }
        .publish(&env);
        Ok(())
    }

    fn set_pool(env: Env, pool: Address) -> Result<(), YieldManagerError> {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        if storage::is_pool_set(&env) {
            return Err(YieldManagerError::PoolAlreadySet);
        }

        storage::set_pool(&env, &pool);

        PoolSet { pool }.publish(&env);
        Ok(())
    }

    fn get_pool(env: Env) -> Address {
        storage::extend_instance_ttl(&env);
        storage::get_pool(&env)
    }

    fn get_vault(env: Env) -> Address {
        storage::extend_instance_ttl(&env);
        storage::get_vault(&env)
    }

    fn get_principal_token(env: Env) -> Address {
        storage::extend_instance_ttl(&env);
        storage::get_principal_token(&env)
    }

    fn get_yield_token(env: Env) -> Address {
        storage::extend_instance_ttl(&env);
        storage::get_yield_token(&env)
    }

    fn get_maturity(env: Env) -> u64 {
        storage::extend_instance_ttl(&env);
        storage::get_maturity(&env)
    }

    fn get_treasury(env: Env) -> Address {
        storage::extend_instance_ttl(&env);
        storage::get_treasury(&env)
    }

    fn get_exchange_rate(env: Env) -> i128 {
        storage::extend_instance_ttl(&env);
        YieldManager::update_exchange_rate(&env)
    }

    fn deposit(env: Env, from: Address, shares_amount: i128) -> Result<(), YieldManagerError> {
        from.require_auth();
        YieldManager::open(&env)?;
        YieldManager::positive(shares_amount)?;
        YieldManager::before_maturity(&env)?;

        let exchange_rate = YieldManager::current_rate(&env)?;
        let mint_amount = YieldManager::assets_for_shares(shares_amount, exchange_rate);

        // YM is the spender (direct invoker), so transfer_from needs no nested
        // require_auth on the depositor.
        let ym = env.current_contract_address();
        token::Client::new(&env, &storage::get_vault(&env)).transfer_from(
            &ym,
            &from,
            &ym,
            &shares_amount,
        );

        YieldManager::mint_pair(&env, &from, &from, mint_amount, exchange_rate);

        Deposit {
            from,
            shares_amount,
            mint_amount,
            exchange_rate,
        }
        .publish(&env);
        Ok(())
    }

    fn redeem_combined(env: Env, from: Address, amount: i128) -> Result<(), YieldManagerError> {
        from.require_auth();
        YieldManager::open(&env)?;
        YieldManager::positive(amount)?;
        YieldManager::before_maturity(&env)?;

        let exchange_rate = YieldManager::current_rate(&env)?;
        let shares_to_return = YieldManager::shares_for_assets(amount, exchange_rate);

        YieldManager::burn_pair(&env, &from, amount, exchange_rate);

        token::Client::new(&env, &storage::get_vault(&env)).transfer(
            &env.current_contract_address(),
            &from,
            &shares_to_return,
        );

        RedeemCombined {
            from,
            amount,
            shares_returned: shares_to_return,
            exchange_rate,
        }
        .publish(&env);
        Ok(())
    }

    /// See the trait doc; the vault deposit names the YM as receiver so the
    /// shares land in custody directly, and the share count is measured here
    /// rather than signed.
    fn deposit_asset(
        env: Env,
        from: Address,
        asset_amount: i128,
        min_tokens_out: i128,
    ) -> Result<i128, YieldManagerError> {
        from.require_auth();
        YieldManager::open(&env)?;
        YieldManager::positive(asset_amount)?;
        YieldManager::before_maturity(&env)?;

        let exchange_rate = YieldManager::current_rate(&env)?;

        let vault_addr = storage::get_vault(&env);
        let ym = env.current_contract_address();

        let vault_token = token::Client::new(&env, &vault_addr);
        let shares_before = vault_token.balance(&ym);
        VaultContractClient::new(&env, &vault_addr).deposit(&asset_amount, &ym, &from, &from);
        let shares_in = vault_token.balance(&ym) - shares_before;
        if shares_in <= 0 {
            return Err(YieldManagerError::VaultDepositFailed);
        }

        let mint_amount = YieldManager::assets_for_shares(shares_in, exchange_rate);
        // Dust that rounds to zero PT is refused rather than minted for nothing.
        YieldManager::positive(mint_amount)?;
        if mint_amount < min_tokens_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        YieldManager::mint_pair(&env, &from, &from, mint_amount, exchange_rate);

        DepositAsset {
            from,
            asset_in: asset_amount,
            shares_in,
            mint_amount,
            exchange_rate,
        }
        .publish(&env);
        Ok(mint_amount)
    }

    /// Asset-out variant of `redeem_combined`: burns the pair, then redeems the
    /// owed shares from YM custody with the vault paying `from` directly.
    fn redeem_combined_to_asset(
        env: Env,
        from: Address,
        amount: i128,
        min_asset_out: i128,
    ) -> Result<i128, YieldManagerError> {
        from.require_auth();
        YieldManager::open(&env)?;
        YieldManager::positive(amount)?;
        YieldManager::before_maturity(&env)?;

        let exchange_rate = YieldManager::current_rate(&env)?;
        let shares_to_return = YieldManager::shares_for_assets(amount, exchange_rate);

        YieldManager::burn_pair(&env, &from, amount, exchange_rate);

        let asset_out = YieldManager::redeem_custody_to(&env, &from, shares_to_return);
        if asset_out < min_asset_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        RedeemToAsset {
            from,
            burned: amount,
            shares_redeemed: shares_to_return,
            asset_out,
            exchange_rate,
        }
        .publish(&env);
        Ok(asset_out)
    }

    /// One vault redemption for both legs; see the trait doc.
    fn exit_expired_to_asset(
        env: Env,
        from: Address,
        max_pt: i128,
        max_shares: i128,
        min_asset_out: i128,
    ) -> Result<i128, YieldManagerError> {
        from.require_auth();
        YieldManager::open(&env)?;
        // Either ceiling may be zero: the caller may hold only PT, or only shares.
        if max_pt < 0 || max_shares < 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        YieldManager::after_maturity(&env)?;

        let vault_addr = storage::get_vault(&env);
        let ym = env.current_contract_address();
        let vault_token = token::Client::new(&env, &vault_addr);

        let (locked_rate, live_rate) = YieldManager::settlement_rates(&env)?;

        // ── PT leg ───────────────────────────────────────────────────────────
        let pt_token = token::Client::new(&env, &storage::get_principal_token(&env));
        let pt_amount = max_pt.min(pt_token.balance(&from));
        let mut shares_total = 0;

        if pt_amount > 0 {
            shares_total +=
                YieldManager::shares_for_principal(&env, pt_amount, locked_rate, live_rate);
            pt_token.burn_from(&ym, &from, &pt_amount);
        }

        // ── Loose shares ─────────────────────────────────────────────────────
        // Whatever the caller is already holding — an LP withdrawal's share leg,
        // a YT yield payout. Moved into custody with a transfer_from against the
        // caller's allowance, which is cheap, so the redemption below covers
        // both legs at once.
        let extra_shares = max_shares.min(vault_token.balance(&from));
        if extra_shares > 0 {
            vault_token.transfer_from(&ym, &from, &ym, &extra_shares);
            shares_total += extra_shares;
        }

        YieldManager::positive(shares_total)?;

        let asset_out = YieldManager::redeem_custody_to(&env, &from, shares_total);
        if asset_out < min_asset_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        RedeemToAsset {
            from,
            burned: pt_amount,
            shares_redeemed: shares_total,
            asset_out,
            exchange_rate: live_rate,
        }
        .publish(&env);
        Ok(asset_out)
    }

    fn distribute_yield(
        env: Env,
        to: Address,
        shares_amount: i128,
    ) -> Result<i128, YieldManagerError> {
        YieldManager::open(&env)?;

        // Only the YT contract can call this
        storage::get_yield_token(&env).require_auth();

        if shares_amount <= 0 {
            return Ok(0);
        }

        let exchange_rate = YieldManager::current_rate(&env)?;

        // Locked: pay the frozen share count's locked-rate asset value at the
        // live rate (fewer shares if the vault kept appreciating); the
        // difference is protocol surplus. See `collect_surplus`.
        let shares_to_send = if storage::is_rate_locked(&env) {
            let live_rate = YieldManager::get_vault_exchange_rate(&env).max(exchange_rate);
            let paid = shares_amount
                .checked_mul(exchange_rate)
                .expect("overflow adjusting yield payout")
                / live_rate;
            YieldManager::bank_surplus(&env, shares_amount - paid);
            paid
        } else {
            shares_amount
        };

        token::Client::new(&env, &storage::get_vault(&env)).transfer(
            &env.current_contract_address(),
            &to,
            &shares_to_send,
        );

        DistributeYield {
            to,
            shares_amount: shares_to_send,
            exchange_rate,
        }
        .publish(&env);
        Ok(shares_to_send)
    }

    fn redeem_principal(env: Env, from: Address, pt_amount: i128) -> Result<(), YieldManagerError> {
        from.require_auth();
        YieldManager::open(&env)?;
        YieldManager::positive(pt_amount)?;
        YieldManager::after_maturity(&env)?;

        let (locked_rate, live_rate) = YieldManager::settlement_rates(&env)?;
        let shares_to_return =
            YieldManager::shares_for_principal(&env, pt_amount, locked_rate, live_rate);

        token::Client::new(&env, &storage::get_principal_token(&env)).burn(&from, &pt_amount);
        token::Client::new(&env, &storage::get_vault(&env)).transfer(
            &env.current_contract_address(),
            &from,
            &shares_to_return,
        );

        RedeemPrincipal {
            from,
            pt_amount,
            shares_returned: shares_to_return,
            exchange_rate: live_rate,
        }
        .publish(&env);
        Ok(())
    }

    fn collect_surplus(env: Env) -> Result<i128, YieldManagerError> {
        YieldManager::open(&env)?;

        let surplus = storage::get_surplus_shares(&env);
        if surplus <= 0 {
            return Ok(0);
        }
        storage::set_surplus_shares(&env, 0);

        let treasury = storage::get_treasury(&env);
        token::Client::new(&env, &storage::get_vault(&env)).transfer(
            &env.current_contract_address(),
            &treasury,
            &surplus,
        );

        SurplusCollected {
            treasury,
            amount: surplus,
        }
        .publish(&env);
        Ok(surplus)
    }
}

// `V` in these callbacks is the AMM's name for vault shares.
#[contractimpl]
impl FlashSwapPtReceiver for YieldManager {
    fn on_flash_receive_pt(
        env: Env,
        yt_out: i128,
        v_from_pool: i128,
        user: Address,
        max_v_in: i128,
        rate_hint: i128,
        amm: Address,
    ) {
        YieldManager::open(&env).unwrap_or_else(|err| panic_with_error!(&env, err));

        // Only the registered pool may drive this callback. This succeeds without a
        // signature when the pool contract is the call's direct invoker (the same
        // mechanism that lets the YM satisfy admin.require_auth() on PT/YT below) —
        // a direct caller impersonating the pool has no way to satisfy this.
        storage::get_pool(&env).require_auth();

        let ym = env.current_contract_address();

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);

        let vault_client = token::Client::new(&env, &vault_addr);
        let pt_client = token::Client::new(&env, &pt_addr);

        // The pool read this rate from this contract to price the trade; see
        // `update_exchange_rate_from`.
        let exchange_rate = YieldManager::update_exchange_rate_from(&env, rate_hint);

        // Vault shares needed to mint yt_out PT+YT (inverse of deposit's mint math).
        let v_to_mint = YieldManager::shares_for_assets(yt_out, exchange_rate);

        // The pool advanced `v_from_pool` as its payment for the yt_out PT it is buying;
        // the user tops up the remainder (the YT price). Guard both against underpricing
        // and against exceeding the user's slippage bound.
        let user_cost = v_to_mint - v_from_pool;
        if user_cost <= 0 {
            panic_with_error!(&env, YieldManagerError::NonPositiveYtCost);
        }
        if user_cost > max_v_in {
            panic_with_error!(&env, YieldManagerError::SlippageExceeded);
        }

        // Pull the slippage bound and refund the excess: max_v_in is the one amount the
        // user can sign without it drifting with pool state. The pull must stay here,
        // authenticated against `user` — it is what stops a direct caller of
        // flash_swap_pt from minting against the V backing other depositors.
        vault_client.transfer(&user, &ym, &max_v_in);
        let refund = max_v_in - user_cost;
        if refund > 0 {
            vault_client.transfer(&ym, &user, &refund);
        }

        // Order matters: backing in (above), then mint, then PT out, so the
        // pair is never unbacked and the delta check below sees only our own
        // mint. Measured against the balance on entry, never against zero:
        // anyone can send PT here, and a check against zero would brick every
        // YT purchase.
        let pt_before = pt_client.balance(&ym);

        // PT to the YM (handed to the pool below), YT to the user.
        YieldManager::mint_pair(&env, &ym, &user, yt_out, exchange_rate);

        // Deliver the PT to the pool — repayment for the advanced V.
        pt_client.transfer(&ym, &amm, &yt_out);

        if pt_client.balance(&ym) != pt_before {
            panic_with_error!(&env, YieldManagerError::PtNotDelivered);
        }

        FlashDeposit {
            user,
            amm,
            yt_out,
            v_to_mint,
            user_cost,
            exchange_rate,
        }
        .publish(&env);
    }
}

#[contractimpl]
impl FlashSwapVReceiver for YieldManager {
    fn on_flash_receive_v(
        env: Env,
        pt_borrowed: i128,
        v_owed: i128,
        user: Address,
        min_v_out: i128,
        rate_hint: i128,
        amm: Address,
    ) {
        YieldManager::open(&env).unwrap_or_else(|err| panic_with_error!(&env, err));

        // Only the registered pool may drive this callback — see on_flash_receive_pt.
        storage::get_pool(&env).require_auth();

        let ym = env.current_contract_address();
        let vault_client = token::Client::new(&env, &storage::get_vault(&env));

        // The pool read this rate from this contract to price the trade; see
        // `update_exchange_rate_from`. The YT burn below takes it as a hint too,
        // so the YT contract never calls back into the YM mid-callback.
        let exchange_rate = YieldManager::update_exchange_rate_from(&env, rate_hint);

        // No user pull here: the router moved the user's `pt_borrowed` YT in before the
        // flash swap began, keeping the exchange rate out of the user's signed auth entry.

        let shares_returned = YieldManager::shares_for_assets(pt_borrowed, exchange_rate);

        // Burn the redeemed pair: PT lent by the AMM, YT moved in by the router.
        // Either leg missing fails the burn — which also stops direct callers.
        YieldManager::burn_pair(&env, &ym, pt_borrowed, exchange_rate);

        if shares_returned < v_owed {
            panic_with_error!(&env, YieldManagerError::RedeemBelowOwed);
        }
        let v_to_user = shares_returned - v_owed;
        if v_to_user < min_v_out {
            panic_with_error!(&env, YieldManagerError::SlippageExceeded);
        }

        vault_client.transfer(&ym, &amm, &v_owed);
        vault_client.transfer(&ym, &user, &v_to_user);

        FlashRedeem {
            user,
            amm,
            pt_borrowed,
            v_owed,
            v_to_user,
            exchange_rate,
        }
        .publish(&env);
    }
}
