use soroban_sdk::{testutils::{Address as _, Ledger}, Address, Env, IntoVal, String, Symbol};

use amm::LiquidityPoolClient;
use factory::{Factory, FactoryClient, Market, WasmHashes};
use mock_vault::{MockVault, MockVaultClient};
use router::RouterContract;
use yield_manager::VaultType;

// The factory deploys the market's contracts from compiled WASMs, exactly as
// production does — so `stellar contract build` must run before these tests.
mod ym_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/yield_manager.wasm");
}
mod pt_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/principal_token.wasm");
}
mod yt_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/yield_token.wasm");
}
mod amm_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/amm.wasm");
}

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
    pub factory: FactoryClient<'a>,
    pub yield_manager: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: LiquidityPoolClient<'a>,
    pub router: Address,
}

impl<'a> IntegrationFixture<'a> {
    pub fn new(env: &'a Env) -> Self {
        env.mock_all_auths();
        // The test budget is cumulative across every invocation in a test
        // (setup included), unlike on-chain where each transaction gets its own
        // budget — so the per-tx default is meaningless here. The resource
        // tests in router_swaps.rs meter individual swaps against real limits.
        env.cost_estimate().budget().reset_unlimited();
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
        // Default rate of 10_000_000 (1.0 in 1e7 fixed-point) gives convert_to_assets(SCALAR_7) = SCALAR_7,
        // so the yield_manager stores an initial exchange rate of SCALAR_7 (1.0).

        // ── Factory ──────────────────────────────────────────────────────────
        // The real factory deploys the whole market (YM, PT, YT, pool) so these
        // tests exercise the same wiring as production.
        let wasm_hashes = WasmHashes {
            pt: env.deployer().upload_contract_wasm(pt_wasm::WASM),
            yt: env.deployer().upload_contract_wasm(yt_wasm::WASM),
            ym: env.deployer().upload_contract_wasm(ym_wasm::WASM),
            amm: env.deployer().upload_contract_wasm(amm_wasm::WASM),
        };
        let factory_addr = env.register(Factory, (&admin, wasm_hashes));
        let factory = FactoryClient::new(env, &factory_addr);

        // ── Market (YM + PT + YT + AMM) ──────────────────────────────────────
        let market = factory.create_market(
            &vault_addr,
            &VaultType::Vault4626,
            &maturity,
            &SCALAR_ROOT,
            &INITIAL_ANCHOR,
            &FEE_RATE_ROOT,
            &LAST_IMPLIED_RATE,
        );
        let pool = LiquidityPoolClient::new(env, &market.pool);

        // ── Router ───────────────────────────────────────────────────────────
        // One global router; it resolves each vault's current market through
        // the factory.
        let router_addr = env.register(RouterContract, (&factory_addr,));

        // ── Fund user ────────────────────────────────────────────────────────
        vault.mint(&user, &1_000_000_000);

        IntegrationFixture {
            env: env.clone(),
            admin,
            user,
            vault,
            factory,
            yield_manager: market.ym,
            pt: market.pt,
            yt: market.yt,
            pool,
            router: router_addr,
        }
    }

    /// Registers a fresh mock vault and creates a market for it through the
    /// factory, mirroring the primary market's params. Funds `self.user` with
    /// the same vault-share balance the primary vault gives.
    pub fn create_market_for_new_vault(&self, symbol: &str) -> (Address, Market) {
        let vault_addr = self.env.register(
            MockVault,
            (&self.admin, String::from_str(&self.env, "Mock Vault"), String::from_str(&self.env, symbol), 7u32),
        );
        let maturity = self.env.ledger().timestamp() + ONE_YEAR_SECS;
        let market = self.factory.create_market(
            &vault_addr,
            &VaultType::Vault4626,
            &maturity,
            &SCALAR_ROOT,
            &INITIAL_ANCHOR,
            &FEE_RATE_ROOT,
            &LAST_IMPLIED_RATE,
        );
        self.env.invoke_contract::<()>(
            &vault_addr,
            &Symbol::new(&self.env, "set_exchange_rate"),
            (10_000_000i128,).into_val(&self.env),
        );
        self.env.invoke_contract::<()>(
            &vault_addr,
            &Symbol::new(&self.env, "mint"),
            (&self.user, 1_000_000_000i128).into_val(&self.env),
        );
        (vault_addr, market)
    }

    /// Rolls the vault's expired market over to `new_maturity` with the same
    /// AMM params as the fixture market.
    pub fn rollover(&self, vault: &Address, new_maturity: u64) -> bool {
        self.factory.rollover_if_expired(
            vault,
            &VaultType::Vault4626,
            &new_maturity,
            &SCALAR_ROOT,
            &INITIAL_ANCHOR,
            &FEE_RATE_ROOT,
            &LAST_IMPLIED_RATE,
        )
    }

    /// Deposit vault shares into yield_manager, returning PT minted.
    pub fn ym_deposit(&self, user: &Address, shares: i128) {
        self.ym_deposit_to(&self.vault.address, &self.yield_manager, user, shares);
    }

    /// Multi-market variant of `ym_deposit`: deposit `vault` shares into `ym`.
    pub fn ym_deposit_to(&self, vault: &Address, ym: &Address, user: &Address, shares: i128) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.env.invoke_contract::<()>(
            vault,
            &Symbol::new(&self.env, "approve"),
            (user, ym, shares, expiry_ledger).into_val(&self.env),
        );
        self.env.invoke_contract::<()>(
            ym,
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
        self.amm_deposit_to(&self.vault.address, &self.pt, &self.pool.address, from, pt_amount, v_amount);
    }

    /// Multi-market variant of `amm_deposit`: seed `pool` with `pt` + `vault` shares.
    pub fn amm_deposit_to(
        &self,
        vault: &Address,
        pt: &Address,
        pool: &Address,
        from: &Address,
        pt_amount: i128,
        v_amount: i128,
    ) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.env.invoke_contract::<()>(
            pt,
            &Symbol::new(&self.env, "approve"),
            (from, pool, pt_amount, expiry_ledger).into_val(&self.env),
        );
        self.env.invoke_contract::<()>(
            vault,
            &Symbol::new(&self.env, "approve"),
            (from, pool, v_amount, expiry_ledger).into_val(&self.env),
        );
        self.env.invoke_contract::<()>(
            pool,
            &Symbol::new(&self.env, "deposit"),
            (from, pt_amount, 0i128, v_amount, 0i128).into_val(&self.env),
        );
    }

    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|l| l.timestamp += seconds);
    }

    /// Router: buy exactly `yt_out` YT, spending at most `max_v_in` vault shares
    /// (V→YT via flash_swap_pt).
    pub fn router_swap_v_for_yt(&self, to: &Address, yt_out: i128, max_v_in: i128) {
        self.router_swap_v_for_yt_on(&self.vault.address, to, yt_out, max_v_in);
    }

    /// Multi-market variant of `router_swap_v_for_yt`.
    pub fn router_swap_v_for_yt_on(&self, vault: &Address, to: &Address, yt_out: i128, max_v_in: i128) {
        self.env.invoke_contract::<()>(
            &self.router,
            &Symbol::new(&self.env, "swap_v_for_yt"),
            (vault, to, yt_out, max_v_in).into_val(&self.env),
        );
    }

    /// Router: sell YT for vault shares (YT→V via flash_swap_v).
    pub fn router_swap_yt_for_v(&self, to: &Address, yt_in: i128, min_v_out: i128) {
        self.router_swap_yt_for_v_on(&self.vault.address, to, yt_in, min_v_out);
    }

    /// Multi-market variant of `router_swap_yt_for_v`.
    pub fn router_swap_yt_for_v_on(&self, vault: &Address, to: &Address, yt_in: i128, min_v_out: i128) {
        self.env.invoke_contract::<()>(
            &self.router,
            &Symbol::new(&self.env, "swap_yt_for_v"),
            (vault, to, yt_in, min_v_out).into_val(&self.env),
        );
    }
}