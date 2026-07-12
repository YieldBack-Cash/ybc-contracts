use soroban_sdk::{token, Address, Env};
use crate::events::{Deposit, DistributeYield, FlashDeposit, FlashRedeem, RedeemCombined, RedeemPrincipal, TokenContractsSet};
use crate::storage;
use amm_interface::{FlashSwapReceiver, FlashSwapVReceiver};
use vault_interface::VaultContractClient;
use defindex_interface::DefindexVaultContractClient;
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
    // Helper function to get exchange rate from vault
    fn get_vault_exchange_rate(env: &Env) -> i128 {
        let vault_addr = storage::get_vault(env);
        let vault_type = storage::get_vault_type(env);

        match vault_type {
            VaultType::Vault4626 => {
                let client = VaultContractClient::new(env, &vault_addr);
                client.convert_to_assets(&SCALAR_7)
            }
            VaultType::VaultDefindex => {
                let client = DefindexVaultContractClient::new(env, &vault_addr);
                let asset_amounts = client.get_asset_amounts_per_shares(&SCALAR_7);
                asset_amounts.get(0).expect("Defindex returned no asset amounts")
            }
        }
    }

    // Refreshes the stored rate before maturity (rate can only increase, and locks
    // once maturity is reached) and returns the resulting current rate.
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
    ) {
        storage::set_admin(&env, &admin);
        storage::set_vault(&env, &vault);
        storage::set_vault_type(&env, vault_type);
        storage::set_maturity(&env, maturity);

        let initial_rate = YieldManager::get_vault_exchange_rate(&env);
        storage::set_exchange_rate(&env, initial_rate);
    }

    fn set_token_contracts(env: Env, pt_addr: Address, yt_addr: Address) -> Result<(), YieldManagerError> {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        // Ensure this can only be called once
        if storage::is_initialized(&env) {
            return Err(YieldManagerError::AlreadyInitialized);
        }

        storage::set_principal_token(&env, &pt_addr);
        storage::set_yield_token(&env, &yt_addr);

        TokenContractsSet { pt: pt_addr, yt: yt_addr }.publish(&env);
        Ok(())
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

        let exchange_rate = YieldManager::update_exchange_rate(&env);

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);

        let mint_amount = shares_amount
            .checked_mul(exchange_rate)
            .expect("overflow computing mint_amount")
            / SCALAR_7;

        // Pull vault shares from depositor into YM via transfer_from (YM is the spender — direct
        // invoker — so no nested require_auth on the depositor's address is needed).
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

        // Burn equal amounts of PT and YT from the caller.
        // YT burn passes the exchange_rate hint so the YT contract does not need to call
        // back into the YM for the rate (which would cause re-entry).
        token::Client::new(&env, &pt_addr).burn(&from, &amount);
        YieldTokenClient::new(&env, &yt_addr).burn_with_rate(&from, &amount, &exchange_rate);

        // Return the corresponding vault shares.
        token::Client::new(&env, &vault_addr)
            .transfer(&env.current_contract_address(), &from, &shares_to_return);

        RedeemCombined { from, amount, shares_returned: shares_to_return, exchange_rate }.publish(&env);
        Ok(())
    }

    fn distribute_yield(env: Env, to: Address, shares_amount: i128) -> Result<(), YieldManagerError> {
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            return Err(YieldManagerError::NotInitialized);
        }

        // Only the YT contract can call this
        let yt_addr = storage::get_yield_token(&env);
        yt_addr.require_auth();

        if shares_amount <= 0 {
            return Ok(());
        }

        let exchange_rate = YieldManager::update_exchange_rate(&env);

        // Transfer vault shares from yield manager to user
        let vault_addr = storage::get_vault(&env);
        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer(
            &env.current_contract_address(),
            &to,
            &shares_amount,
        );

        DistributeYield { to, shares_amount, exchange_rate }.publish(&env);
        Ok(())
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

        // Check maturity has passed
        let maturity = storage::get_maturity(&env);
        let current_time = env.ledger().timestamp();
        if current_time < maturity {
            return Err(YieldManagerError::MaturityNotReached);
        }

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);

        let exchange_rate = YieldManager::update_exchange_rate(&env);
        if exchange_rate == 0 {
            return Err(YieldManagerError::ExchangeRateZero);
        }
        let shares_to_return = pt_amount
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_to_return")
            / exchange_rate;

        // Burn PT tokens from user
        let pt_token_client = token::Client::new(&env, &pt_addr);
        pt_token_client.burn(&from, &pt_amount);

        // Transfer vault shares back to user
        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer(
            &env.current_contract_address(),
            &from,
            &shares_to_return,
        );

        RedeemPrincipal { from, pt_amount, shares_returned: shares_to_return, exchange_rate }.publish(&env);
        Ok(())
    }
}

#[cfg(feature = "contract")]
#[contractimpl]
impl FlashSwapReceiver for YieldManager {
    fn on_flash_receive(env: Env, pt_borrowed: i128, user: Address, v_in: i128, min_yt_out: i128, amm: Address) {
        storage::extend_instance_ttl(&env);

        if !storage::is_initialized(&env) {
            panic!("Token contracts not initialized");
        }

        let ym = env.current_contract_address();

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);

        let vault_client = token::Client::new(&env, &vault_addr);
        let pt_client = token::Client::new(&env, &pt_addr);

        // Fetch exchange rate before any minting.
        let exchange_rate = YieldManager::update_exchange_rate(&env);
        assert!(exchange_rate > 0, "exchange rate is zero");

        let yt_minted = v_in
            .checked_mul(exchange_rate)
            .expect("overflow computing yt_minted")
            / SCALAR_7;
        assert!(yt_minted >= min_yt_out, "yt below minimum");

        // Pull vault shares from user directly into YM.
        // user is an account address so user.require_auth() resolves without callbacks.
        vault_client.transfer(&user, &ym, &v_in);

        // Mint PT to YM (repays AMM) and YT to user.
        // YM is the direct caller of both mint functions so admin.require_auth() is satisfied.
        let principal_client = PrincipalTokenClient::new(&env, &pt_addr);
        let yt_token_client = YieldTokenClient::new(&env, &yt_addr);
        principal_client.mint(&ym, &yt_minted);
        yt_token_client.mint(&user, &yt_minted, &exchange_rate);

        // Repay AMM with borrowed PT plus all newly minted PT.
        let pt_to_repay = pt_borrowed
            .checked_add(yt_minted)
            .expect("overflow computing pt_to_repay");
        pt_client.transfer(&ym, &amm, &pt_to_repay);

        assert_eq!(pt_client.balance(&ym), 0, "YM leaked PT");

        FlashDeposit { user, amm, v_in, pt_borrowed, yt_minted, exchange_rate }.publish(&env);
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

        // Pull the user's YT — pairs 1:1 with the PT the AMM lent us.
        yt_client.transfer_with_rate(&user, &ym, &pt_borrowed, &exchange_rate);

        let shares_returned = pt_borrowed
            .checked_mul(SCALAR_7)
            .expect("overflow computing shares_returned")
            / exchange_rate;

        // Burn YM's PT and YT (received from AMM + pulled from user).
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