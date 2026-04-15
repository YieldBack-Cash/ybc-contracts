use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::TokenClient,
    Address, Env, IntoVal, String, Symbol,
};

use crate::{YieldManager, VaultType};
use principal_token::PrincipalToken;
use yield_token::YieldToken;
use mock_vault::{MockVault, MockVaultClient};

pub struct YieldManagerTest {
    pub env: Env,
    pub admin: Address,
    pub user1: Address,
    pub user2: Address,
    pub vault_addr: Address,
    pub yield_manager: Address,
    pub pt: Address,
    pub yt: Address,
    pub maturity: u64,
}

impl YieldManagerTest {
    pub fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        let vault_addr = env.register(
            MockVault,
            (
                &admin,
                String::from_str(&env, "Mock Vault Token"),
                String::from_str(&env, "MVT"),
                7u32,
            ),
        );

        let current_time = env.ledger().timestamp();
        let maturity = current_time + 1000;

        let yield_manager_id = env.register(YieldManager, (&admin, &vault_addr, VaultType::Vault4626, maturity));

        let pt_id = env.register(
            PrincipalToken,
            (
                &yield_manager_id,
                String::from_str(&env, "Principal Token"),
                String::from_str(&env, "PT"),
                7u32,
            ),
        );

        let yt_id = env.register(
            YieldToken,
            (
                &yield_manager_id,
                String::from_str(&env, "Yield Token"),
                String::from_str(&env, "YT"),
                7u32,
            ),
        );

        env.invoke_contract::<()>(
            &yield_manager_id,
            &Symbol::new(&env, "set_token_contracts"),
            (&pt_id, &yt_id).into_val(&env),
        );

        YieldManagerTest {
            env,
            admin,
            user1,
            user2,
            vault_addr,
            yield_manager: yield_manager_id,
            pt: pt_id,
            yt: yt_id,
            maturity,
        }
    }

    pub fn mint_vault_shares(&self, to: &Address, amount: i128) {
        let vault_client = MockVaultClient::new(&self.env, &self.vault_addr);
        vault_client.mint(to, &amount);
    }

    pub fn set_vault_exchange_rate(&self, rate: i128) {
        let vault_client = MockVaultClient::new(&self.env, &self.vault_addr);
        vault_client.set_exchange_rate(&rate);
    }

    pub fn vault_balance(&self, user: &Address) -> i128 {
        let token = TokenClient::new(&self.env, &self.vault_addr);
        token.balance(user)
    }

    pub fn get_pt_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.pt,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    pub fn get_yt_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yt,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    pub fn deposit(&self, user: &Address, shares: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_manager,
            &Symbol::new(&self.env, "deposit"),
            (user, shares).into_val(&self.env),
        );
    }

    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|li| {
            li.timestamp += seconds;
        });
    }
}