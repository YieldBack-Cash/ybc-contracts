use soroban_sdk::{token, Address, Env};
use crate::storage;
use amm_interface::{FlashSwapReceiver, FlashSwapVReceiver};
use vault_interface::VaultContractClient;
use defindex_interface::DefindexVaultContractClient;
use yield_manager_interface::{YieldManagerTrait, VaultType};
use principal_token_interface::PrincipalTokenClient;
use yield_token_interface::YieldTokenClient;

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
                client.convert_to_assets(&1i128)
            }
            VaultType::VaultDefindex => {
                let client = DefindexVaultContractClient::new(env, &vault_addr);
                let asset_amounts = client.get_asset_amounts_per_shares(&1i128);
                asset_amounts.get(0).unwrap()
            }
        }
    }

    // Update maturity before maturity (exchange rate for users locks after maturity)
    // Rate can only increase
    fn update_exchange_rate(env: &Env) {
        if storage::is_rate_locked(env) {
            return;
        }

        let maturity = storage::get_maturity(env);
        let current_time = env.ledger().timestamp();

        // Get current vault rate using the helper function
        let new_rate = YieldManager::get_vault_exchange_rate(env);

        // Get the currently stored rate
        let stored_rate = storage::get_exchange_rate(env);

        // Only update if the new rate is higher
        if new_rate > stored_rate {
            storage::set_exchange_rate(env, new_rate);
        }

        // If we've reached or passed maturity, lock the rate
        if current_time >= maturity {
            storage::set_rate_locked(env);
        }
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

        // Fetch and store the initial exchange rate from the vault using the helper function
        let initial_rate = YieldManager::get_vault_exchange_rate(&env);
        storage::set_exchange_rate(&env, initial_rate);
    }

    fn set_token_contracts(env: Env, pt_addr: Address, yt_addr: Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        // Ensure this can only be called once
        if storage::is_initialized(&env) {
            panic!("Token contracts already initialized");
        }

        storage::set_principal_token(&env, &pt_addr);
        storage::set_yield_token(&env, &yt_addr);
        storage::set_initialized(&env);
    }

    fn get_vault(env: Env) -> Address {
        storage::get_vault(&env)
    }

    fn get_principal_token(env: Env) -> Address {
        storage::get_principal_token(&env)
    }

    fn get_yield_token(env: Env) -> Address {
        storage::get_yield_token(&env)
    }

    fn get_maturity(env: Env) -> u64 {
        storage::get_maturity(&env)
    }

    fn get_exchange_rate(env: Env) -> i128 {
        // Update the stored exchange rate (if before maturity)
        YieldManager::update_exchange_rate(&env);
        // Return the stored rate
        storage::get_exchange_rate(&env)
    }

    fn deposit(env: Env, from: Address, shares_amount: i128) {
        from.require_auth();

        if shares_amount <= 0 {
            panic!("Amount must be positive");
        }

        // Update the stored exchange rate (if before maturity)
        YieldManager::update_exchange_rate(&env);

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);
        
        let exchange_rate = storage::get_exchange_rate(&env);
        let mint_amount = shares_amount * exchange_rate / 10_000_000;

        // Pull vault shares from depositor into YM via transfer_from (YM is the spender — direct
        // invoker — so no nested require_auth on the depositor's address is needed).
        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer_from(
            &env.current_contract_address(),
            &from,
            &env.current_contract_address(),
            &shares_amount,
        );

        // Mint PT tokens to user (shares * exchange_rate) using type-safe client
        let pt_client = PrincipalTokenClient::new(&env, &pt_addr);
        pt_client.mint(&from, &mint_amount);

        // Mint YT tokens to user (shares * exchange_rate) using unified client
        let yt_client = YieldTokenClient::new(&env, &yt_addr);
        yt_client.mint(&from, &mint_amount, &exchange_rate);
    }

    fn redeem(env: Env, from: Address, amount: i128) {
        from.require_auth();

        if !storage::is_initialized(&env) {
            panic!("Token contracts not initialized");
        }

        if amount <= 0 {
            panic!("Amount must be positive");
        }

        // After maturity PT holders must use redeem_principal; combining PT+YT
        // here would burn YT for no extra shares.
        let maturity = storage::get_maturity(&env);
        if env.ledger().timestamp() >= maturity {
            panic!("Maturity reached; use redeem_principal");
        }

        YieldManager::update_exchange_rate(&env);

        let exchange_rate = storage::get_exchange_rate(&env);
        if exchange_rate == 0 {
            panic!("Exchange rate is zero");
        }
        let shares_to_return = amount * 10_000_000 / exchange_rate;

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
    }

    fn distribute_yield(env: Env, to: Address, shares_amount: i128) {
        // Only the YT contract can call this
        let yt_addr = storage::get_yield_token(&env);
        yt_addr.require_auth();

        if shares_amount <= 0 {
            return;
        }

        // Update the stored exchange rate (if before maturity)
        YieldManager::update_exchange_rate(&env);

        // Transfer vault shares from yield manager to user
        let vault_addr = storage::get_vault(&env);
        let vault_token_client = token::Client::new(&env, &vault_addr);
        vault_token_client.transfer(
            &env.current_contract_address(),
            &to,
            &shares_amount,
        );
    }

    fn redeem_principal(env: Env, from: Address, pt_amount: i128) {
        from.require_auth();

        if pt_amount <= 0 {
            panic!("Amount must be positive");
        }

        // Check maturity has passed
        let maturity = storage::get_maturity(&env);
        let current_time = env.ledger().timestamp();
        if current_time < maturity {
            panic!("Maturity not reached");
        }

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);

        // Get the stored exchange rate (locked at maturity)
        // Multiply by scale (1e7) to reverse the scaling applied during deposit
        let exchange_rate = storage::get_exchange_rate(&env);
        let shares_to_return = pt_amount * 10_000_000 / exchange_rate;

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
    }
}

#[cfg(feature = "contract")]
#[contractimpl]
impl FlashSwapReceiver for YieldManager {
    fn on_flash_receive(env: Env, pt_borrowed: i128, user: Address, v_in: i128, min_yt_out: i128, amm: Address) {
        let ym = env.current_contract_address();

        let vault_addr = storage::get_vault(&env);
        let pt_addr = storage::get_principal_token(&env);
        let yt_addr = storage::get_yield_token(&env);

        let vault_client = token::Client::new(&env, &vault_addr);
        let pt_client = token::Client::new(&env, &pt_addr);

        // Fetch exchange rate before any minting.
        YieldManager::update_exchange_rate(&env);
        let exchange_rate = storage::get_exchange_rate(&env);
        assert!(exchange_rate > 0, "exchange rate is zero");

        let yt_minted = v_in * exchange_rate / 10_000_000;
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
        pt_client.transfer(&ym, &amm, &(pt_borrowed + yt_minted));

        assert_eq!(pt_client.balance(&ym), 0, "YM leaked PT");
    }
}

#[cfg(feature = "contract")]
#[contractimpl]
impl FlashSwapVReceiver for YieldManager {
    fn on_flash_receive_v(env: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128, amm: Address) {
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
        YieldManager::update_exchange_rate(&env);
        let exchange_rate = storage::get_exchange_rate(&env);
        assert!(exchange_rate > 0, "exchange rate is zero");

        // Pull the user's YT — pairs 1:1 with the PT the AMM lent us.
        yt_client.transfer_with_rate(&user, &ym, &pt_borrowed, &exchange_rate);

        let shares_returned = pt_borrowed * 10_000_000 / exchange_rate;

        // Burn YM's PT and YT (received from AMM + pulled from user).
        pt_client.burn(&ym, &pt_borrowed);
        yt_client.burn_with_rate(&ym, &pt_borrowed, &exchange_rate);

        assert!(shares_returned >= v_owed, "redeem yielded less V than owed to pool");
        let v_to_user = shares_returned - v_owed;
        assert!(v_to_user >= min_v_out, "v out below minimum");

        vault_client.transfer(&ym, &amm, &v_owed);
        vault_client.transfer(&ym, &user, &v_to_user);
    }
}