use soroban_sdk::{token, Address, Env};
use crate::events::{Deposit, DistributeYield, FlashDeposit, FlashRedeem, PoolSet, RedeemCombined, RedeemPrincipal, SurplusCollected, TokenContractsSet};
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
    fn update_exchange_rate(env: &Env) -> i128 {
        if storage::is_rate_locked(env) {
            return storage::get_exchange_rate(env);
        }

        let maturity = storage::get_maturity(env);
        let current_time = env.ledger().timestamp();

        let new_rate = YieldManager::get_vault_exchange_rate(env);
        let stored_rate = storage::get_exchange_rate(env);

        let current_rate = if new_rate > stored_rate {
            storage::set_exchange_rate(env, new_rate);
            new_rate
        } else {
            stored_rate
        };

        if current_time >= maturity {
            storage::set_rate_locked(env);
        }

        current_rate
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
    fn on_flash_receive_pt(env: Env, yt_out: i128, v_from_pool: i128, user: Address, max_v_in: i128, amm: Address) {
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

        let exchange_rate = YieldManager::update_exchange_rate(&env);
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
    fn on_flash_receive_v(env: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128, amm: Address) {
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

        // Fetch the exchange rate before any YT operations. All YT calls below use this
        // rate as a hint so the YT contract never calls back into the YM, which would
        // trigger re-entry while on_flash_receive_v is executing.
        let exchange_rate = YieldManager::update_exchange_rate(&env);
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