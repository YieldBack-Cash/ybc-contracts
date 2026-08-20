use soroban_sdk::{token, Address, Env};
use crate::events::{Deposit, DepositAsset, DistributeYield, FlashDeposit, FlashRedeem, PoolSet, RedeemCombined, RedeemPrincipal, RedeemToAsset, SurplusCollected, TokenContractsSet};
use crate::storage;
use amm_interface::{FlashSwapPtReceiver, FlashSwapVReceiver};
use vault_interface::VaultContractClient;
use yield_manager_interface::{YieldManagerTrait, VaultType, YieldManagerError};
use principal_token_interface::PrincipalTokenClient;
use yield_token_interface::YieldTokenClient;

const SCALAR_7: i128 = 1_0000000;

#[cfg(feature = "contract")]
use soroban_sdk::{contract, contractimpl};

#[cfg(feature = "contract")]
#[contract]
pub struct YieldManager;

#[cfg(feature = "contract")]
impl YieldManager {
    /// Current vault rate, expressed as assets per SCALAR_7 shares (1e7-scaled).
    fn get_vault_exchange_rate(env: &Env) -> i128 {
        let vault_addr = storage::get_vault(env);
        let vault_type = storage::get_vault_type(env);

        match vault_type {
            VaultType::Vault4626 => {
                let client = VaultContractClient::new(env, &vault_addr);
                client.convert_to_assets(&SCALAR_7)
            }
        }
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

        // The locked check stays ahead of the read: once maturity has passed the
        // vault is never consulted at all. Inlining this as a single delegation
        // would evaluate the argument first and read it regardless.
        YieldManager::update_exchange_rate_from(env, YieldManager::get_vault_exchange_rate(env))
    }

    /// As [`YieldManager::update_exchange_rate`], but with the current rate
    /// supplied by the caller rather than read here.
    ///
    /// For callers that have ALREADY loaded it in this same transaction — today
    /// only the flash-swap callbacks, where the AMM reads the rate to price the
    /// trade and hands it down. The rate cannot move within a transaction, so
    /// reading it again returns the same number for the price of a full round
    /// trip (pre-maturity this reaches through to the underlying lending pool).
    ///
    /// Every piece of policy stays here: the non-decreasing floor still applies and
    /// the maturity lock is still set. The caller supplies only the observation.
    ///
    /// The figure the pool passes is now THIS contract's own rate making a round
    /// trip: the pool obtains it from `get_exchange_rate` to price the trade, which
    /// has already committed the ratchet, so re-applying it here is idempotent and
    /// the floor below is a no-op. That is a stronger guarantee than the one this
    /// function used to rest on, which was that the pool passed a raw vault reading
    /// and never an already-high-water-marked one — a precondition that could only
    /// be documented, not enforced, and whose failure direction (a value too HIGH)
    /// would have ratcheted the stored rate up permanently. The pool can no longer
    /// supply any value except the one already in storage.
    ///
    /// The pairing is guaranteed by construction rather than checked: `create_market`
    /// threads one `vault` value into both this contract and the pool's share-token
    /// slot, and `set_pool` is one-shot, so the two cannot be re-pointed afterwards.
    /// Anything that ever lets them be deployed apart has to re-establish that.
    fn update_exchange_rate_from(env: &Env, vault_rate: i128) -> i128 {
        if storage::is_rate_locked(env) {
            return storage::get_exchange_rate(env);
        }
        assert!(vault_rate > 0, "vault rate must be positive");

        let maturity = storage::get_maturity(env);
        let current_time = env.ledger().timestamp();

        let stored_rate = storage::get_exchange_rate(env);

        let current_rate = if vault_rate > stored_rate {
            storage::set_exchange_rate(env, vault_rate);
            vault_rate
        } else {
            stored_rate
        };

        if current_time >= maturity {
            storage::set_rate_locked(env);
        }

        current_rate
    }

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
}

#[cfg(feature = "contract")]
#[contractimpl]
impl YieldManagerTrait for YieldManager {
    fn __constructor(
        env: Env,
        admin: Address,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
        treasury: Address,
    ) {
        storage::set_admin(&env, &admin);
        storage::set_vault(&env, &vault);
        storage::set_vault_type(&env, vault_type);
        storage::set_maturity(&env, maturity);
        storage::set_treasury(&env, &treasury);

        let initial_rate = YieldManager::get_vault_exchange_rate(&env);
        storage::set_exchange_rate(&env, initial_rate);
    }

    fn set_token_contracts(env: Env, pt_addr: Address, yt_addr: Address) -> Result<(), YieldManagerError> {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        if storage::is_initialized(&env) {
            return Err(YieldManagerError::AlreadyInitialized);
        }

        storage::set_principal_token(&env, &pt_addr);
        storage::set_yield_token(&env, &yt_addr);

        TokenContractsSet { pt: pt_addr, yt: yt_addr }.publish(&env);
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
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }

        if shares_amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }

        // Minting into a matured market is pointless at best: PT would mint at
        // the locked rate but redeem at the live rate, losing the difference.
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() >= maturity {
            return Err(YieldManagerError::MaturityReached);
        }

        let exchange_rate = YieldManager::update_exchange_rate(&env);

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);

        let mint_amount = shares_amount
            .checked_mul(exchange_rate)
            .expect("overflow computing mint_amount")
            / SCALAR_7;

        // YM is the spender (direct invoker), so transfer_from needs no nested
        // require_auth on the depositor.
        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer_from(
            &env.current_contract_address(),
            &from,
            &env.current_contract_address(),
            &shares_amount,
        );

        let pt_client = PrincipalTokenClient::new(&env, &pt_addr);
        pt_client.mint(&from, &mint_amount);

        let yt_client = YieldTokenClient::new(&env, &yt_addr);
        yt_client.mint(&from, &mint_amount, &exchange_rate);

        Deposit { from, shares_amount, mint_amount, exchange_rate }.publish(&env);
        Ok(())
    }

    fn redeem_combined(env: Env, from: Address, amount: i128) -> Result<(), YieldManagerError> {
        from.require_auth();
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }

        if amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }

        // After maturity PT holders must use redeem_principal; combining PT+YT
        // here would burn YT for no extra shares.
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() >= maturity {
            return Err(YieldManagerError::MaturityReached);
        }

        let exchange_rate = YieldManager::update_exchange_rate(&env);
        if exchange_rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }
        let shares_to_return = amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_to_return")
            / exchange_rate;

        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);
        let vault_addr = storage::get_vault(&env);

        // The rate hint stops the YT contract calling back into the YM for it
        // (re-entry while the YM is on the call stack is rejected by the host).
        token::Client::new(&env, &pt_addr).burn(&from, &amount);
        YieldTokenClient::new(&env, &yt_addr).burn_with_rate(&from, &amount, &exchange_rate);

        token::Client::new(&env, &vault_addr)
            .transfer(&env.current_contract_address(), &from, &shares_to_return);

        RedeemCombined { from, amount, shares_returned: shares_to_return, exchange_rate }.publish(&env);
        Ok(())
    }

    /// Asset-in variant of `deposit`. The vault deposit names the YM as
    /// receiver, so the freshly minted shares land straight in YM custody —
    /// exactly where `deposit` would have moved them — and the user's signed
    /// entries (this call, the vault deposit, the nested asset transfer) carry
    /// only caller-chosen arguments. The share count, which nobody can predict
    /// at signing time, is measured here under no one's signature.
    fn deposit_asset(env: Env, from: Address, asset_amount: i128, min_tokens_out: i128) -> Result<i128, YieldManagerError> {
        from.require_auth();
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }
        if asset_amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() >= maturity {
            return Err(YieldManagerError::MaturityReached);
        }

        let exchange_rate = YieldManager::update_exchange_rate(&env);

        let vault_addr = storage::get_vault(&env);
        let ym = env.current_contract_address();

        let vault_token = token::Client::new(&env, &vault_addr);
        let shares_before = vault_token.balance(&ym);
        VaultContractClient::new(&env, &vault_addr).deposit(&asset_amount, &ym, &from, &from);
        let shares_in = vault_token.balance(&ym) - shares_before;
        if shares_in <= 0 {
            return Err(YieldManagerError::VaultDepositFailed);
        }

        let mint_amount = shares_in
            .checked_mul(exchange_rate)
            .expect("overflow computing mint_amount")
            / SCALAR_7;
        if mint_amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        if mint_amount < min_tokens_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);
        PrincipalTokenClient::new(&env, &pt_addr).mint(&from, &mint_amount);
        YieldTokenClient::new(&env, &yt_addr).mint(&from, &mint_amount, &exchange_rate);

        DepositAsset { from, asset_in: asset_amount, shares_in, mint_amount, exchange_rate }.publish(&env);
        Ok(mint_amount)
    }

    /// Asset-out variant of `redeem_combined`: burns the pair, then redeems the
    /// owed shares from YM custody with the vault paying `from` directly.
    fn redeem_combined_to_asset(env: Env, from: Address, amount: i128, min_asset_out: i128) -> Result<i128, YieldManagerError> {
        from.require_auth();
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }
        if amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        // Post-maturity, PT must go through redeem_principal_to_asset instead —
        // same rule as redeem_combined.
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() >= maturity {
            return Err(YieldManagerError::MaturityReached);
        }

        let exchange_rate = YieldManager::update_exchange_rate(&env);
        if exchange_rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }
        let shares_to_return = amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_to_return")
            / exchange_rate;

        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);

        // Burn amounts are caller-chosen, so the user's signed burn entries are
        // fixed; the rate hint stops the YT calling back into the YM mid-call.
        token::Client::new(&env, &pt_addr).burn(&from, &amount);
        YieldTokenClient::new(&env, &yt_addr).burn_with_rate(&from, &amount, &exchange_rate);

        let asset_out = YieldManager::redeem_custody_to(&env, &from, shares_to_return);
        if asset_out < min_asset_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        RedeemToAsset { from, burned: amount, shares_redeemed: shares_to_return, asset_out, exchange_rate }.publish(&env);
        Ok(asset_out)
    }

    /// Asset-out variant of `redeem_principal`, sized by clamp rather than by
    /// exact amount: burns `min(max_pt, balance)` through the PT allowance the
    /// caller granted the YM. That is what lets an exit redeem "everything,
    /// including the PT an LP withdrawal just produced" — the measured figure
    /// goes through `burn_from`, never through the user's signature. Face-value
    /// and surplus accounting are identical to `redeem_principal`.
    fn redeem_principal_to_asset(env: Env, from: Address, max_pt: i128, min_asset_out: i128) -> Result<i128, YieldManagerError> {
        from.require_auth();
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }
        if max_pt <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() < maturity {
            return Err(YieldManagerError::MaturityNotReached);
        }

        let pt_addr = storage::get_principal_token(&env);
        let pt_token = token::Client::new(&env, &pt_addr);

        let pt_amount = max_pt.min(pt_token.balance(&from));
        if pt_amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }

        let locked_rate = YieldManager::update_exchange_rate(&env);
        if locked_rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }
        // Same face-value rule as redeem_principal: redeem at the live rate,
        // floored at the locked rate, with the freed difference recorded as
        // protocol surplus for collect_surplus.
        let exchange_rate = YieldManager::get_vault_exchange_rate(&env).max(locked_rate);

        let shares_to_return = pt_amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_to_return")
            / exchange_rate;
        let shares_at_locked = pt_amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_at_locked")
            / locked_rate;
        let freed = shares_at_locked - shares_to_return;
        if freed > 0 {
            storage::set_surplus_shares(&env, storage::get_surplus_shares(&env) + freed);
        }

        // YM is the direct invoker, so spender auth is automatic; the amount is
        // covered by (and consumes) the caller's allowance.
        pt_token.burn_from(&env.current_contract_address(), &from, &pt_amount);

        let asset_out = YieldManager::redeem_custody_to(&env, &from, shares_to_return);
        if asset_out < min_asset_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        RedeemToAsset { from, burned: pt_amount, shares_redeemed: shares_to_return, asset_out, exchange_rate }.publish(&env);
        Ok(asset_out)
    }

    /// PT redemption and loose-share conversion in a SINGLE vault redemption.
    /// See the interface docs for why that matters; in short, a redemption is a
    /// lending-pool submission and two of them do not fit in one transaction
    /// alongside an LP withdrawal.
    fn exit_expired_to_asset(env: Env, from: Address, max_pt: i128, max_shares: i128, min_asset_out: i128) -> Result<i128, YieldManagerError> {
        from.require_auth();
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }
        if max_pt < 0 || max_shares < 0 {
            return Err(YieldManagerError::InvalidAmount);
        }
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() < maturity {
            return Err(YieldManagerError::MaturityNotReached);
        }

        let vault_addr = storage::get_vault(&env);
        let ym = env.current_contract_address();
        let vault_token = token::Client::new(&env, &vault_addr);

        let locked_rate = YieldManager::update_exchange_rate(&env);
        if locked_rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }
        // Face-value rule, identical to redeem_principal: pay at the live rate
        // floored at the locked one, and bank the difference as surplus.
        let exchange_rate = YieldManager::get_vault_exchange_rate(&env).max(locked_rate);

        // ── PT leg ───────────────────────────────────────────────────────────
        let pt_addr = storage::get_principal_token(&env);
        let pt_token = token::Client::new(&env, &pt_addr);
        let pt_amount = max_pt.min(pt_token.balance(&from));
        let mut shares_total = 0;

        if pt_amount > 0 {
            let shares_for_pt = pt_amount
                .checked_mul(SCALAR_7)
                .expect("overflow computing shares_for_pt")
                / exchange_rate;
            let shares_at_locked = pt_amount
                .checked_mul(SCALAR_7)
                .expect("overflow computing shares_at_locked")
                / locked_rate;
            let freed = shares_at_locked - shares_for_pt;
            if freed > 0 {
                storage::set_surplus_shares(&env, storage::get_surplus_shares(&env) + freed);
            }
            pt_token.burn_from(&ym, &from, &pt_amount);
            shares_total += shares_for_pt;
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

        if shares_total <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }

        let asset_out = YieldManager::redeem_custody_to(&env, &from, shares_total);
        if asset_out < min_asset_out {
            return Err(YieldManagerError::SlippageExceeded);
        }

        RedeemToAsset { from, burned: pt_amount, shares_redeemed: shares_total, asset_out, exchange_rate }.publish(&env);
        Ok(asset_out)
    }

    fn distribute_yield(env: Env, to: Address, shares_amount: i128) -> Result<i128, YieldManagerError> {
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }

        // Only the YT contract can call this
        let yt_addr = storage::get_yield_token(&env);
        yt_addr.require_auth();

        if shares_amount <= 0 {
            return Ok(0);
        }

        let exchange_rate = YieldManager::update_exchange_rate(&env);

        // The YT accrues yield as a share count frozen against rates up to the
        // locked rate. Positions freeze in ASSET value at maturity, so once the
        // rate is locked a claim pays the shares that asset value buys at the
        // live rate — fewer shares when the vault has kept appreciating. The
        // difference is post-maturity interest and belongs to the protocol.
        let shares_to_send = if storage::is_rate_locked(&env) {
            let live_rate = YieldManager::get_vault_exchange_rate(&env).max(exchange_rate);
            if live_rate == 0 {
                return Err(YieldManagerError::ExchangeRateZero);
            }
            let paid = shares_amount
                .checked_mul(exchange_rate)
                .expect("overflow adjusting yield payout")
                / live_rate;
            let freed = shares_amount - paid;
            if freed > 0 {
                storage::set_surplus_shares(&env, storage::get_surplus_shares(&env) + freed);
            }
            paid
        } else {
            shares_amount
        };

        let vault_addr = storage::get_vault(&env);
        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer(
            &env.current_contract_address(),
            &to,
            &shares_to_send,
        );

        DistributeYield { to, shares_amount: shares_to_send, exchange_rate }.publish(&env);
        Ok(shares_to_send)
    }

    fn redeem_principal(env: Env, from: Address, pt_amount: i128) -> Result<(), YieldManagerError> {
        from.require_auth();
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }

        if pt_amount <= 0 {
            return Err(YieldManagerError::InvalidAmount);
        }

        let maturity = storage::get_maturity(&env);
        let current_time = env.ledger().timestamp();
        if current_time < maturity {
            return Err(YieldManagerError::MaturityNotReached);
        }

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);

        let locked_rate = YieldManager::update_exchange_rate(&env);
        if locked_rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }

        // Redeem at the live vault rate so PT always pays exactly face value
        // in assets; the shares that keep appreciating after maturity stay in
        // the YM as protocol surplus. Floored at the locked rate so a vault
        // rate dip can't pay out more shares than were reserved at maturity.
        let exchange_rate = YieldManager::get_vault_exchange_rate(&env).max(locked_rate);

        let shares_to_return = pt_amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_to_return")
            / exchange_rate;

        // This PT's backing was reserved at the locked rate; redeeming at a
        // higher live rate frees the difference — the post-maturity interest
        // the redeemer forgoes ("you snooze you lose"). Record it for
        // collect_surplus.
        let shares_at_locked = pt_amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_at_locked")
            / locked_rate;
        let freed = shares_at_locked - shares_to_return;
        if freed > 0 {
            storage::set_surplus_shares(&env, storage::get_surplus_shares(&env) + freed);
        }

        let pt_token_client = token::Client::new(&env, &pt_addr);
        pt_token_client.burn(&from, &pt_amount);

        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer(
            &env.current_contract_address(),
            &from,
            &shares_to_return,
        );

        RedeemPrincipal { from, pt_amount, shares_returned: shares_to_return, exchange_rate }.publish(&env);
        Ok(())
    }

    fn collect_surplus(env: Env) -> Result<i128, YieldManagerError> {
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }

        let surplus = storage::get_surplus_shares(&env);
        if surplus <= 0 {
            return Ok(0);
        }
        storage::set_surplus_shares(&env, 0);

        let treasury = storage::get_treasury(&env);
        let vault_addr = storage::get_vault(&env);
        token::Client::new(&env, &vault_addr).transfer(
            &env.current_contract_address(),
            &treasury,
            &surplus,
        );

        SurplusCollected { treasury, amount: surplus }.publish(&env);
        Ok(surplus)
    }
}

#[cfg(feature = "contract")]
#[contractimpl]
impl FlashSwapPtReceiver for YieldManager {
    fn on_flash_receive_pt(env: Env, yt_out: i128, v_from_pool: i128, user: Address, max_v_in: i128, vault_rate: i128, amm: Address) {
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            panic!("Token contracts not initialized");
        }

        // Only the registered pool may drive this callback. This succeeds without a
        // signature when the pool contract is the call's direct invoker (the same
        // mechanism that lets the YM satisfy admin.require_auth() on PT/YT below) —
        // a direct caller impersonating the pool has no way to satisfy this.
        storage::get_pool(&env).require_auth();

        let ym = env.current_contract_address();

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);

        let vault_client = token::Client::new(&env, &vault_addr);
        let pt_client = token::Client::new(&env, &pt_addr);

        // Supplied by the pool, which read it from THIS contract to price this trade —
        // see `update_exchange_rate_from` for why that is the only safe source.
        let exchange_rate = YieldManager::update_exchange_rate_from(&env, vault_rate);
        assert!(exchange_rate > 0, "exchange rate is zero");

        // Vault shares needed to mint yt_out PT+YT (inverse of deposit's mint math).
        let v_to_mint = yt_out
            .checked_mul(SCALAR_7)
            .expect("overflow computing v_to_mint")
            / exchange_rate;

        // The pool advanced `v_from_pool` as its payment for the yt_out PT it is buying;
        // the user tops up the remainder (the YT price). Guard both against underpricing
        // and against exceeding the user's slippage bound.
        let user_cost = v_to_mint
            .checked_sub(v_from_pool)
            .expect("underflow computing user_cost");
        assert!(user_cost > 0, "non-positive YT cost");
        assert!(user_cost <= max_v_in, "cost exceeds max_v_in");

        // Pull the slippage bound and refund the excess: max_v_in is the one amount the
        // user can sign without it drifting with pool state. The pull must stay here,
        // authenticated against `user` — it is what stops a direct caller of
        // flash_swap_pt from minting against the V backing other depositors.
        vault_client.transfer(&user, &ym, &max_v_in);
        let refund = max_v_in - user_cost;
        if refund > 0 {
            vault_client.transfer(&ym, &user, &refund);
        }

        // YM is the direct caller of both mints, satisfying admin.require_auth().
        let principal_client = PrincipalTokenClient::new(&env, &pt_addr);
        let yt_token_client = YieldTokenClient::new(&env, &yt_addr);
        principal_client.mint(&ym, &yt_out);
        yt_token_client.mint(&user, &yt_out, &exchange_rate);

        // Deliver the PT to the pool — repayment for the advanced V.
        pt_client.transfer(&ym, &amm, &yt_out);

        assert_eq!(pt_client.balance(&ym), 0, "YM leaked PT");

        FlashDeposit { user, amm, yt_out, v_to_mint, user_cost, exchange_rate }.publish(&env);
    }
}

#[cfg(feature = "contract")]
#[contractimpl]
impl FlashSwapVReceiver for YieldManager {
    fn on_flash_receive_v(env: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128, vault_rate: i128, amm: Address) {
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            panic!("Token contracts not initialized");
        }

        // Only the registered pool may drive this callback — see on_flash_receive_pt.
        storage::get_pool(&env).require_auth();

        let ym = env.current_contract_address();

        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);
        let vault_addr = storage::get_vault(&env);

        let pt_client = token::Client::new(&env, &pt_addr);
        let yt_client = YieldTokenClient::new(&env, &yt_addr);
        let vault_client = token::Client::new(&env, &vault_addr);

        // Supplied by the pool, which read it from THIS contract to price this trade —
        // see `update_exchange_rate_from` for why that is the only safe source. All
        // YT calls below take it as a hint too, so the YT contract never calls back
        // into the YM, which would re-enter while this callback is executing.
        let exchange_rate = YieldManager::update_exchange_rate_from(&env, vault_rate);
        assert!(exchange_rate > 0, "exchange rate is zero");

        // No user pull here: the router moved the user's `pt_borrowed` YT in before the
        // flash swap began, keeping the exchange rate out of the user's signed auth entry.

        let shares_returned = pt_borrowed
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_returned")
            / exchange_rate;

        // Burn the redeemed pair: PT lent by the AMM, YT moved in by the router.
        // Either leg missing fails the burn — which also stops direct callers.
        pt_client.burn(&ym, &pt_borrowed);
        yt_client.burn_with_rate(&ym, &pt_borrowed, &exchange_rate);

        assert!(shares_returned >= v_owed, "redeem yielded less V than owed to pool");
        let v_to_user = shares_returned - v_owed;
        assert!(v_to_user >= min_v_out, "v out below minimum");

        vault_client.transfer(&ym, &amm, &v_owed);
        vault_client.transfer(&ym, &user, &v_to_user);

        FlashRedeem { user, amm, pt_borrowed, v_owed, v_to_user, exchange_rate }.publish(&env);
    }
}