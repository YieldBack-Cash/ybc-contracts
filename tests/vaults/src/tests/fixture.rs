//! One YBC market on one real vault adapter binary, created through the real
//! factory exactly as production does.

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec, Address, Env, IntoVal, String, Symbol,
};

use factory::{Factory, FactoryClient, FeeConfig, WasmHashes};
use router::RouterContract;
use yield_manager_interface::YieldManagerClient;
use yield_token_interface::YieldTokenClient;

use vault_testkit::protocols::blend::{self as blend_protocol, pool, BlendFixture};
use vault_testkit::protocols::xoxno::{
    HubAssetKey, MockController, MockControllerClient, HUB_ID, SPOKE_ID,
};
/// The SEP-56 client, generated from `vault_common::sep56::Sep56Vault`: the
/// same declaration the adapters are compile-checked against. Nothing in these
/// tests may call a function YBC would not.
pub use vault_testkit::VaultClient;

// The YBC contracts the factory deploys, as compiled: `stellar contract build`
// must run before these tests.
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

// The vault adapters, as built from the ybc-vaults workspace.
pub mod blend_vault_wasm {
    soroban_sdk::contractimport!(file = "../../wasms/blend_vault.wasm");
}
pub mod xoxno_vault_wasm {
    soroban_sdk::contractimport!(file = "../../wasms/xoxno_vault.wasm");
}

// AMM market params (1e7-scaled APYs), as in the integration suite.
const CURRENT_APY: i128 = 1_000_000;
const APY_MIN: i128 = 200_000;
const APY_MAX: i128 = 2_000_000;
const FEE_APY: i128 = 100_000;

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;
pub const ONE_DAY_SECS: u64 = 24 * 3600;

pub enum Backend {
    /// The Blend pool the vault supplies into.
    Blend { pool: Address },
    /// The mock XOXNO controller the vault supplies into.
    Xoxno { controller: Address },
}

#[allow(dead_code)]
pub struct VaultStack<'a> {
    pub env: Env,
    pub admin: Address,
    pub user: Address,
    pub underlying: Address,
    pub vault: Address,
    pub backend: Backend,
    pub factory: FactoryClient<'a>,
    pub yield_manager: Address,
    pub pt: Address,
    pub yt: Address,
    pub amm: Address,
    pub router: Address,
    pub maturity: u64,
}

impl<'a> VaultStack<'a> {
    fn prepare(env: &Env) -> (Address, Address) {
        // Non-root auth: the mock controller and the Blend pool both move a
        // depositor's tokens from inside a nested contract frame.
        env.mock_all_auths_allowing_non_root_auth();
        env.cost_estimate().budget().reset_unlimited();
        env.ledger().with_mut(|l| {
            l.timestamp = 1_000_000;
            l.sequence_number = 100;
        });
        (Address::generate(env), Address::generate(env))
    }

    /// A market on `blend_vault.wasm` over a real Blend pool at ~50%
    /// utilisation, underlying priced at $1.
    pub fn blend(env: &'a Env) -> Self {
        let (admin, user) = Self::prepare(env);

        let blnd = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let usdc = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let protocol = BlendFixture::deploy(env, &admin, &blnd, &usdc);

        let underlying = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        StellarAssetClient::new(env, &underlying).mint(&admin, &20_000_000_0000000i128);
        let pool = blend_protocol::deploy_pool(env, &protocol, &admin, &underlying);

        let vault = env.register(
            blend_vault_wasm::WASM,
            (
                &admin,
                &pool,
                &underlying,
                &blnd,
                String::from_str(env, "Blend Vault Share"),
                String::from_str(env, "bVS"),
            ),
        );

        Self::finish(env, admin, user, underlying, vault, Backend::Blend { pool })
    }

    /// A market on `xoxno_vault.wasm` over the mock controller at index RAY.
    pub fn xoxno(env: &'a Env) -> Self {
        let (admin, user) = Self::prepare(env);

        let underlying = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        let controller = env.register(MockController, (underlying.clone(), HUB_ID));

        let vault = env.register(
            xoxno_vault_wasm::WASM,
            (
                &controller,
                &underlying,
                HUB_ID,
                SPOKE_ID,
                String::from_str(env, "XOXNO Vault Share"),
                String::from_str(env, "xvS"),
            ),
        );

        Self::finish(
            env,
            admin,
            user,
            underlying,
            vault,
            Backend::Xoxno { controller },
        )
    }

    fn finish(
        env: &'a Env,
        admin: Address,
        user: Address,
        underlying: Address,
        vault: Address,
        backend: Backend,
    ) -> Self {
        let maturity = env.ledger().timestamp() + ONE_YEAR_SECS;

        let wasm_hashes = WasmHashes {
            pt: env.deployer().upload_contract_wasm(pt_wasm::WASM),
            yt: env.deployer().upload_contract_wasm(yt_wasm::WASM),
            ym: env.deployer().upload_contract_wasm(ym_wasm::WASM),
            amm: env.deployer().upload_contract_wasm(amm_wasm::WASM),
        };
        let fee_config = FeeConfig {
            treasury: Address::generate(env),
            reserve_fee_rate: 0,
        };
        let factory_addr = env.register(Factory, (&admin, wasm_hashes, fee_config));
        let factory = FactoryClient::new(env, &factory_addr);

        // The factory's YM constructor probes `convert_to_assets` on the vault.
        // Neither adapter needs a bootstrap deposit for that any more.
        let market = factory.create_market(
            &admin,
            &vault,
            &maturity,
            &CURRENT_APY,
            &APY_MIN,
            &APY_MAX,
            &FEE_APY,
        );

        let router = env.register(RouterContract, (&factory_addr,));

        VaultStack {
            env: env.clone(),
            admin,
            user,
            underlying,
            vault,
            backend,
            factory,
            yield_manager: market.ym,
            pt: market.pt,
            yt: market.yt,
            amm: market.pool,
            router,
            maturity,
        }
    }

    // ── clients ─────────────────────────────────────────────────────────────

    pub fn vault_client(&self) -> VaultClient<'_> {
        VaultClient::new(&self.env, &self.vault)
    }

    pub fn ym(&self) -> YieldManagerClient<'_> {
        YieldManagerClient::new(&self.env, &self.yield_manager)
    }

    fn token(&self, id: &Address) -> TokenClient<'_> {
        TokenClient::new(&self.env, id)
    }

    // ── balances ────────────────────────────────────────────────────────────

    pub fn underlying_balance(&self, who: &Address) -> i128 {
        self.token(&self.underlying).balance(who)
    }

    pub fn vault_shares(&self, who: &Address) -> i128 {
        self.token(&self.vault).balance(who)
    }

    pub fn pt_balance(&self, who: &Address) -> i128 {
        self.token(&self.pt).balance(who)
    }

    pub fn yt_balance(&self, who: &Address) -> i128 {
        self.token(&self.yt).balance(who)
    }

    // ── actions ─────────────────────────────────────────────────────────────

    pub fn mint_underlying(&self, to: &Address, amount: i128) {
        StellarAssetClient::new(&self.env, &self.underlying).mint(to, &amount);
    }

    /// Deposit into the vault directly, receiving shares.
    pub fn vault_deposit(&self, user: &Address, amount: i128) -> i128 {
        self.vault_client().deposit(&amount, user, user, user)
    }

    pub fn vault_approve(&self, owner: &Address, spender: &Address, amount: i128) {
        let expiry = self.env.ledger().sequence() + 1000;
        self.token(&self.vault)
            .approve(owner, spender, &amount, &expiry);
    }

    /// Full share path: underlying → vault shares → YM → PT + YT.
    pub fn setup_yt_position(&self, user: &Address, amount: i128) {
        self.mint_underlying(user, amount);
        let shares = self.vault_deposit(user, amount);
        self.vault_approve(user, &self.yield_manager, shares);
        self.ym().deposit(user, &shares);
    }

    pub fn claim_yield(&self, user: &Address) -> i128 {
        YieldTokenClient::new(&self.env, &self.yt).claim_yield(user)
    }

    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|l| {
            l.timestamp += seconds;
            l.sequence_number += (seconds / 5).max(1) as u32;
        });
    }

    /// Makes the backing protocol's rate move.
    ///
    /// Blend accrues real interest against the seeded borrow but only writes a
    /// new `b_rate` on a pool interaction, so a small repayment pokes it after
    /// `advance_time`. The mock controller has no borrowers; its index is
    /// raised by half a percent per call, with the cash to back it.
    pub fn accrue_interest(&self) {
        match &self.backend {
            Backend::Blend { pool } => {
                pool::Client::new(&self.env, pool).submit(
                    &self.admin,
                    &self.admin,
                    &self.admin,
                    &vec![
                        &self.env,
                        pool::Request {
                            address: self.underlying.clone(),
                            amount: 1_000_000i128,
                            request_type: 5, // repay
                        },
                    ],
                );
            }
            Backend::Xoxno { controller } => {
                let c = MockControllerClient::new(&self.env, controller);
                let key = HubAssetKey {
                    asset: self.underlying.clone(),
                    hub_id: HUB_ID,
                };
                let before = self.vault_client().total_assets();
                let index = c.get_market_index(&key).supply_index;
                c.set_supply_index(&(index + index / 200));
                let after = self.vault_client().total_assets();
                if after > before {
                    self.mint_underlying(controller, after - before);
                }
            }
        }
    }

    /// `router.zap_asset_for_split`: underlying in, PT + YT out, in one call.
    /// The router refuses a zero minimum, so the floor is one stroop.
    pub fn router_zap_asset_for_split(&self, user: &Address, asset_in: i128) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.router,
            &Symbol::new(&self.env, "zap_asset_for_split"),
            (&self.vault, self.maturity, user, asset_in, 1i128).into_val(&self.env),
        )
    }

    /// `router.zap_split_for_asset`: PT + YT in, underlying out, in one call.
    pub fn router_zap_split_for_asset(&self, user: &Address, amount: i128) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.router,
            &Symbol::new(&self.env, "zap_split_for_asset"),
            (&self.vault, self.maturity, user, amount, 1i128).into_val(&self.env),
        )
    }
}
