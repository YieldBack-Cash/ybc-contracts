use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, IntoVal, String, Symbol};

use amm::{LiquidityPool, LiquidityPoolClient};
use mock_vault::{MockVault, MockVaultClient};
use principal_token::PrincipalToken;
use router::RouterContract;
use yield_manager::{YieldManager, VaultType};
use yield_token::YieldToken;

// AMM market params
const SCALAR_ROOT: i128 = 250_000_000;
const FEE_RATE_ROOT: i128 = 500_000;
const INITIAL_ANCHOR: i128 = 11_000_000;
const LAST_IMPLIED_RATE: i128 = 1_000_000;

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

pub struct IntegrationFixture<'a> {
    pub env: Env,
    pub admin: Address,
    pub user: Address,
    pub vault: MockVaultClient<'a>,
    pub yield_manager: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: LiquidityPoolClient<'a>,
    pub router: Address,
    pub maturity: u64,
}

impl<'a> IntegrationFixture<'a> {
    pub fn new(env: &'a Env) -> Self {
        env.mock_all_auths();
        env.ledger().with_mut(|l| l.timestamp = 1_000_000);

        let admin = Address::generate(env);
        let user = Address::generate(env);
        let maturity = env.ledger().timestamp() + ONE_YEAR_SECS;

        // ── Vault ────────────────────────────────────────────────────────────
        let vault_addr = env.register(
            MockVault,
            (&admin, String::from_str(env, "Mock Vault"), String::from_str(env, "MVT"), 7u32),
        );
        let vault = MockVaultClient::new(env, &vault_addr);
        // Default rate of 10_000_000 (1.0 in 1e7 fixed point) is intentional —
        // yield_manager expects convert_to_assets(1) to return the 1e7-scaled rate.

        // ── Yield Manager ────────────────────────────────────────────────────
        let ym_addr = env.register(YieldManager, (&admin, &vault_addr, VaultType::Vault4626, maturity));

        // ── PT and YT ────────────────────────────────────────────────────────
        // PT registered first → lower counter address (required by AMM)
        let pt_addr = env.register(
            PrincipalToken,
            (&ym_addr, String::from_str(env, "Principal Token"), String::from_str(env, "PT"), 7u32),
        );
        let yt_addr = env.register(
            YieldToken,
            (&ym_addr, String::from_str(env, "Yield Token"), String::from_str(env, "YT"), 7u32),
        );

        env.invoke_contract::<()>(
            &ym_addr,
            &Symbol::new(env, "set_token_contracts"),
            (&pt_addr, &yt_addr).into_val(env),
        );

        // ── AMM ──────────────────────────────────────────────────────────────
        // PT is token_a, vault shares are token_b
        let pool_addr = env.register(
            LiquidityPool,
            (&pt_addr, &vault_addr, maturity, SCALAR_ROOT, INITIAL_ANCHOR, FEE_RATE_ROOT, LAST_IMPLIED_RATE),
        );
        let pool = LiquidityPoolClient::new(env, &pool_addr);

        // ── Router ───────────────────────────────────────────────────────────
        let router_addr = env.register(RouterContract, (&pool_addr, &ym_addr));

        // ── Fund user ────────────────────────────────────────────────────────
        vault.mint(&user, &1_000_000_000);

        IntegrationFixture { env: env.clone(), admin, user, vault, yield_manager: ym_addr, pt: pt_addr, yt: yt_addr, pool, router: router_addr, maturity }
    }

    /// Deposit vault shares into yield_manager, returning PT minted.
    pub fn ym_deposit(&self, user: &Address, shares: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_manager,
            &Symbol::new(&self.env, "deposit"),
            (user, shares).into_val(&self.env),
        );
    }

    pub fn pt_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.pt,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    pub fn yt_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yt,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    /// Approve and deposit PT + vault shares into the AMM.
    pub fn amm_deposit(&self, from: &Address, pt_amount: i128, v_amount: i128) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.env.invoke_contract::<()>(
            &self.pt,
            &Symbol::new(&self.env, "approve"),
            (from, &self.pool.address, pt_amount, expiry_ledger).into_val(&self.env),
        );
        self.vault.approve(from, &self.pool.address, &v_amount, &expiry_ledger);
        self.pool.deposit(from, &pt_amount, &0, &v_amount, &0);
    }

    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|l| l.timestamp += seconds);
    }

    /// Router: buy YT by spending vault shares (V→YT via flash_swap_pt).
    pub fn router_swap_v_for_yt(&self, to: &Address, v_in: i128, min_yt_out: i128) {
        self.env.invoke_contract::<()>(
            &self.router,
            &Symbol::new(&self.env, "swap_v_for_yt"),
            (to, v_in, min_yt_out).into_val(&self.env),
        );
    }

    /// Router: sell YT for vault shares (YT→V via flash_swap_v).
    pub fn router_swap_yt_for_v(&self, to: &Address, yt_in: i128, min_v_out: i128) {
        self.env.invoke_contract::<()>(
            &self.router,
            &Symbol::new(&self.env, "swap_yt_for_v"),
            (to, yt_in, min_v_out).into_val(&self.env),
        );
    }
}