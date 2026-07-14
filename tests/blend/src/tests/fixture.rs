use soroban_sdk::{token::StellarAssetClient, Address, Env, IntoVal, String, Symbol};
use soroban_sdk::testutils::{Address as _, Ledger};

use amm::{LiquidityPool, LiquidityPoolClient};
use principal_token::PrincipalToken;
use router::RouterContract;
use yield_manager::{YieldManager, VaultType};
use yield_token::YieldToken;

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

const SCALAR_ROOT: i128 = 250_000_000;
const FEE_RATE_ROOT: i128 = 500_000;
const INITIAL_ANCHOR: i128 = 11_000_000;
const LAST_IMPLIED_RATE: i128 = 1_000_000;

mod fee_vault {
    soroban_sdk::contractimport!(file = "../../wasms/fee_vault_v2.wasm");
}

/// Inline mock Blend pool — implements the subset of the Blend pool interface
/// that the fee vault reads: `get_reserve` and `get_config`.
pub mod mockpool {
    use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

    const BRATE: Symbol = symbol_short!("b_rate");
    const CONFIG: Symbol = symbol_short!("config");
    const DATA: Symbol = symbol_short!("data");
    const BACKSTOP_RATE: Symbol = symbol_short!("backstop");

    #[derive(Clone, Debug)]
    #[contracttype]
    pub struct Reserve {
        pub asset: Address,
        pub config: ReserveConfig,
        pub data: ReserveData,
        pub scalar: i128,
    }

    #[derive(Clone, Debug, Default)]
    #[contracttype]
    pub struct ReserveConfig {
        pub index: u32,
        pub decimals: u32,
        pub c_factor: u32,
        pub l_factor: u32,
        pub util: u32,
        pub max_util: u32,
        pub r_base: u32,
        pub r_one: u32,
        pub r_two: u32,
        pub r_three: u32,
        pub reactivity: u32,
        pub supply_cap: i128,
        pub enabled: bool,
    }

    #[derive(Clone, Debug, Default)]
    #[contracttype]
    pub struct ReserveData {
        pub d_rate: i128,
        pub b_rate: i128,
        pub ir_mod: i128,
        pub b_supply: i128,
        pub d_supply: i128,
        pub backstop_credit: i128,
        pub last_time: u64,
    }

    #[derive(Clone, Debug)]
    #[contracttype]
    pub struct PoolConfig {
        pub oracle: Address,
        pub min_collateral: i128,
        pub bstop_rate: u32,
        pub status: u32,
        pub max_positions: u32,
    }

    #[contract]
    pub struct MockPool;

    #[contractimpl]
    impl MockPool {
        pub fn set_b_rate(e: Env, b_rate: i128) {
            e.storage().instance().set(&BRATE, &b_rate);
        }

        pub fn set_backstop_rate(e: Env, bstop_rate: u32) {
            e.storage().instance().set(&BACKSTOP_RATE, &bstop_rate);
        }

        /// Replace reserve data wholesale; clears any override b_rate.
        pub fn set_data(e: Env, data: ReserveData) {
            if e.storage().instance().has(&BRATE) {
                e.storage().instance().remove(&BRATE);
            }
            e.storage().instance().set(&DATA, &data);
        }

        pub fn set_config(e: Env, config: ReserveConfig) {
            e.storage().instance().set(&CONFIG, &config);
        }

        pub fn get_reserve(e: Env, reserve: Address) -> Reserve {
            let mut data: ReserveData = e
                .storage()
                .instance()
                .get(&DATA)
                .unwrap_or_default();
            if let Some(b_rate) = e.storage().instance().get(&BRATE) {
                data.b_rate = b_rate;
            }
            Reserve {
                asset: reserve,
                config: e
                    .storage()
                    .instance()
                    .get(&CONFIG)
                    .unwrap_or_default(),
                data,
                scalar: 1_0000000,
            }
        }

        /// Note: only `bstop_rate` is meaningful for the fee vault.
        pub fn get_config(e: Env) -> PoolConfig {
            PoolConfig {
                oracle: e.current_contract_address(),
                min_collateral: 0,
                bstop_rate: e.storage().instance().get(&BACKSTOP_RATE).unwrap_or(0),
                status: 0,
                max_positions: 4,
            }
        }
    }

    pub fn register_mock_pool_with_b_rate(e: &Env, b_rate: i128) -> MockPoolClient {
        let addr = e.register(MockPool {}, ());
        let client = MockPoolClient::new(e, &addr);
        client.set_b_rate(&b_rate);
        client
    }
}

pub struct BlendFixture<'a> {
    pub env: Env,
    pub admin: Address,
    pub user: Address,
    /// Fee vault WASM backed by the mock Blend pool.
    pub fee_vault: fee_vault::Client<'a>,
    /// Mock Blend pool — call `set_b_rate` to simulate yield accrual.
    pub blend_pool: mockpool::MockPoolClient<'a>,
    /// Underlying asset (Stellar Asset Contract).
    pub asset: Address,
    pub yield_manager: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: LiquidityPoolClient<'a>,
    pub router: Address,
    pub maturity: u64,
}

impl<'a> BlendFixture<'a> {
    /// Deploy every contract needed for an end-to-end Blend integration test.
    ///
    /// Chain:  mock Blend pool  →  fee vault WASM
    ///                          →  yield manager  →  PT / YT
    ///                                            →  AMM  →  router
    pub fn new(env: &'a Env) -> Self {
        env.mock_all_auths_allowing_non_root_auth();
        env.ledger().with_mut(|l| l.timestamp = 1_000_000);

        let admin = Address::generate(env);
        let user = Address::generate(env);
        let maturity = env.ledger().timestamp() + ONE_YEAR_SECS;

        // ── Underlying asset ─────────────────────────────────────────────────
        let asset = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();

        // ── Mock Blend pool ──────────────────────────────────────────────────
        // Initial b_rate 1.1 (1_100_000_000_000 in 12-decimal fixed-point).
        let blend_pool =
            mockpool::register_mock_pool_with_b_rate(env, 1_100_000_000_000);

        // ── Fee vault WASM ───────────────────────────────────────────────────
        let fee_vault_addr = env.register(
            fee_vault::WASM,
            (
                admin.clone(),
                blend_pool.address.clone(),
                asset.clone(),
                Option::<Address>::None,
            ),
        );
        let fee_vault = fee_vault::Client::new(env, &fee_vault_addr);

        // ── Yield manager ────────────────────────────────────────────────────
        let ym_addr = env.register(
            YieldManager,
            (&admin, &fee_vault_addr, VaultType::Vault4626, maturity),
        );

        // ── PT and YT ────────────────────────────────────────────────────────
        let pt_addr = env.register(
            PrincipalToken,
            (
                &ym_addr,
                String::from_str(env, "Principal Token"),
                String::from_str(env, "PT"),
                7u32,
            ),
        );
        let yt_addr = env.register(
            YieldToken,
            (
                &ym_addr,
                String::from_str(env, "Yield Token"),
                String::from_str(env, "YT"),
                7u32,
            ),
        );

        env.invoke_contract::<()>(
            &ym_addr,
            &Symbol::new(env, "set_token_contracts"),
            (&pt_addr, &yt_addr).into_val(env),
        );

        // ── AMM ──────────────────────────────────────────────────────────────
        // token_a = PT, token_b = fee vault shares
        let pool_addr = env.register(
            LiquidityPool,
            (
                &pt_addr,
                &fee_vault_addr,
                maturity,
                SCALAR_ROOT,
                INITIAL_ANCHOR,
                FEE_RATE_ROOT,
                LAST_IMPLIED_RATE,
                &ym_addr,
            ),
        );
        let pool = LiquidityPoolClient::new(env, &pool_addr);

        // ── Router ───────────────────────────────────────────────────────────
        let router_addr = env.register(RouterContract, (&pool_addr, &ym_addr));

        BlendFixture {
            env: env.clone(),
            admin,
            user,
            fee_vault,
            blend_pool,
            asset,
            yield_manager: ym_addr,
            pt: pt_addr,
            yt: yt_addr,
            pool,
            router: router_addr,
            maturity,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Simulate Blend yield accrual by updating the mock pool b_rate.
    pub fn set_b_rate(&self, b_rate: i128) {
        self.blend_pool.set_b_rate(&b_rate);
    }

    /// Mint underlying asset to `to` (only works while `env.mock_all_auths()` is active).
    pub fn mint_asset(&self, to: &Address, amount: i128) {
        StellarAssetClient::new(&self.env, &self.asset).mint(to, &amount);
    }

    /// Approve the yield manager to spend `shares` fee vault shares from `user`,
    /// then call `yield_manager.deposit(user, shares)`.
    pub fn ym_deposit(&self, user: &Address, shares: i128) {
        let expiry = self.env.ledger().sequence() + 1000;
        self.fee_vault.approve(user, &self.yield_manager, &shares, &expiry);
        self.env.invoke_contract::<()>(
            &self.yield_manager,
            &Symbol::new(&self.env, "deposit"),
            (user, shares).into_val(&self.env),
        );
    }

    /// Approve and deposit PT + fee vault shares into the AMM.
    pub fn amm_deposit(&self, from: &Address, pt_amount: i128, v_amount: i128) {
        let expiry = self.env.ledger().sequence() + 1000;
        self.env.invoke_contract::<()>(
            &self.pt,
            &Symbol::new(&self.env, "approve"),
            (from, &self.pool.address, pt_amount, expiry).into_val(&self.env),
        );
        self.fee_vault.approve(from, &self.pool.address, &v_amount, &expiry);
        self.pool.deposit(from, &pt_amount, &0, &v_amount, &0);
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

    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|l| l.timestamp += seconds);
    }
}