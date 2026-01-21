use crate::YieldToken;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, String, Symbol,
};

// Import contracts from the workspace
use principal_token::PrincipalToken;
use yield_manager::YieldManager;
use yield_manager_interface::VaultType;
use vault_interface::VaultContractClient;

const VAULT_WASM: &[u8] = include_bytes!("../../../../../wasms/vault.wasm");

/// Shared test fixture for YieldToken tests
pub struct YieldTokenTest<'a> {
    pub env: Env,
    pub user1: Address,
    pub user2: Address,
    pub vault_client: TokenClient<'a>,
    pub vault_address: Address,
    pub yield_manager: Address,
    pub yield_token: Address,
    pub pt: Address,
    pub underlying_asset: TokenClient<'a>,
    pub maturity: u64,
}

impl<'a> YieldTokenTest<'a> {
    pub fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        // Create underlying asset
        let underlying_admin = Address::generate(&env);
        let underlying_asset_addr = env.register_stellar_asset_contract_v2(underlying_admin.clone());
        let underlying_asset = TokenClient::new(&env, &underlying_asset_addr.address());

        // Deploy vault from WASM with constructor parameters (asset, decimals_offset)
        let vault_address = env.register(VAULT_WASM, (&underlying_asset.address, &0u32));
        let vault_client = TokenClient::new(&env, &vault_address);

        // Set maturity to 1000 seconds from now
        let current_time = env.ledger().timestamp();
        let maturity = current_time + 1000;

        // Deploy yield manager
        let yield_manager_id = env.register(
            YieldManager,
            (&admin, &vault_address, &VaultType::Vault4626, &maturity),
        );

        // Mint underlying assets to test depositor
        let test_depositor = Address::generate(&env);
        let underlying_admin_client = StellarAssetClient::new(&env, &underlying_asset.address);
        underlying_admin_client.mint(&test_depositor, &1_000_000_0000000i128);

        // Deposit to vault to get shares using VaultContractClient
        let vault_contract_client = VaultContractClient::new(&env, &vault_address);
        vault_contract_client.deposit(
            &1_000_000_0000000i128,
            &test_depositor,
            &test_depositor,
            &test_depositor,
        );

        // Transfer vault shares to yield manager for distributing yield
        vault_client.transfer(&test_depositor, &yield_manager_id, &1_000_000_0000000i128);

        // Deploy PT token
        let pt_id = env.register(
            PrincipalToken,
            (
                &yield_manager_id,
                &String::from_str(&env, "Principal Token"),
                &String::from_str(&env, "PT"),
                &7u32, // decimals for 1e7
            ),
        );

        // Deploy YT token with decimal parameter
        let yt_id = env.register(
            YieldToken,
            (
                &yield_manager_id,
                &7u32,
                &String::from_str(&env, "Yield Token"),
                &String::from_str(&env, "YT"),
            ),
        );

        // Set token contracts in yield manager
        env.invoke_contract::<()>(
            &yield_manager_id,
            &Symbol::new(&env, "set_token_contracts"),
            (&pt_id, &yt_id).into_val(&env),
        );

        YieldTokenTest {
            env,
            user1,
            user2,
            vault_client,
            vault_address,
            yield_manager: yield_manager_id,
            yield_token: yt_id,
            pt: pt_id,
            underlying_asset,
            maturity,
        }
    }

    pub fn mint_yt(&self, to: &Address, amount: i128, exchange_rate: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_token,
            &Symbol::new(&self.env, "mint"),
            (to, amount, exchange_rate).into_val(&self.env),
        );
    }

    pub fn get_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    pub fn get_user_index(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "user_index"),
            (user,).into_val(&self.env),
        )
    }

    pub fn get_accrued_yield(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "accrued_yield"),
            (user,).into_val(&self.env),
        )
    }

    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|li| {
            li.timestamp += seconds;
        });
    }

    pub fn get_exchange_rate(&self) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_manager,
            &Symbol::new(&self.env, "get_exchange_rate"),
            ().into_val(&self.env),
        )
    }

    pub fn claim_yield(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "claim_yield"),
            (user,).into_val(&self.env),
        )
    }

    pub fn transfer(&self, from: &Address, to: &Address, amount: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_token,
            &Symbol::new(&self.env, "transfer"),
            (from, to, amount).into_val(&self.env),
        );
    }

    pub fn burn(&self, from: &Address, amount: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_token,
            &Symbol::new(&self.env, "burn"),
            (from, amount).into_val(&self.env),
        );
    }

    pub fn get_total_supply(&self) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "total_supply"),
            ().into_val(&self.env),
        )
    }

    pub fn get_decimals(&self) -> u32 {
        self.env.invoke_contract::<u32>(
            &self.yield_token,
            &Symbol::new(&self.env, "decimals"),
            ().into_val(&self.env),
        )
    }

    pub fn get_name(&self) -> String {
        self.env.invoke_contract::<String>(
            &self.yield_token,
            &Symbol::new(&self.env, "name"),
            ().into_val(&self.env),
        )
    }

    pub fn get_symbol(&self) -> String {
        self.env.invoke_contract::<String>(
            &self.yield_token,
            &Symbol::new(&self.env, "symbol"),
            ().into_val(&self.env),
        )
    }
}
