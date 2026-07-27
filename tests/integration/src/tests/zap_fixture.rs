//! Shared fixture for the base-asset zap tests.
//!
//! Deliberately built on `standard_vault` (OpenZeppelin's SEP-56
//! implementation) rather than `mock_vault`. The mock fakes its exchange rate
//! through a setter and holds no underlying asset, so it can say nothing about
//! whether the protocol talks to a standards-compliant vault correctly — and
//! exercising the zaps against a SEP-56 shim written alongside them would bake
//! any misreading of the standard into both sides at once.
//!
//! Here the rate is real: `total_assets` is just the vault's balance of the
//! underlying, so the only way to move it is to give the vault more assets.

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env, IntoVal, String, Symbol,
};

use amm::LiquidityPoolClient;
use factory::{Factory, FactoryClient, FeeConfig, WasmHashes};
use router::{RouterClient, RouterContract};
use standard_vault::{StandardVault, StandardVaultClient};
use yield_manager::VaultType;

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

const CURRENT_APY: i128 = 1_000_000; // 10%
const APY_MIN: i128 = 200_000; // 2%
const APY_MAX: i128 = 2_000_000; // 20%
const FEE_APY: i128 = 100_000; // 1%

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;
pub const USER_ASSET: i128 = 10_000_000_000;
pub const POOL_PT: i128 = 1_500_000_000;
pub const POOL_V: i128 = 1_500_000_000;

pub struct ZapFixture<'a> {
    pub env: Env,
    pub admin: Address,
    pub user: Address,
    pub asset: Address,
    pub vault: Address,
    pub ym: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: LiquidityPoolClient<'a>,
    pub router: RouterClient<'a>,
    pub maturity: u64,
}

impl<'a> ZapFixture<'a> {
    pub fn new(env: &'a Env) -> Self {
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        env.ledger().with_mut(|l| l.timestamp = 1_000_000);

        let admin = Address::generate(env);
        let user = Address::generate(env);
        let maturity = env.ledger().timestamp() + ONE_YEAR_SECS;

        let asset = env
            .register_stellar_asset_contract_v2(admin.clone())
            .address();
        // Offset 0 keeps OpenZeppelin's virtual-share cushion out of the
        // arithmetic, so a failure means the protocol got something wrong rather
        // than the inflation-attack mitigation shifting a rounding boundary.
        let vault = env.register(
            StandardVault,
            (
                &asset,
                0u32,
                String::from_str(env, "Standard Vault"),
                String::from_str(env, "SVT"),
            ),
        );

        token::StellarAssetClient::new(env, &asset).mint(&user, &USER_ASSET);

        let wasm_hashes = WasmHashes {
            pt: env.deployer().upload_contract_wasm(pt_wasm::WASM),
            yt: env.deployer().upload_contract_wasm(yt_wasm::WASM),
            ym: env.deployer().upload_contract_wasm(ym_wasm::WASM),
            amm: env.deployer().upload_contract_wasm(amm_wasm::WASM),
        };
        // Reserve fee rate 0 keeps the treasury out of the asset-conservation
        // sums; the fee-remit paths have their own coverage in the AMM tests.
        let fee_config = FeeConfig {
            treasury: Address::generate(env),
            reserve_fee_rate: 0,
        };
        let factory =
            FactoryClient::new(env, &env.register(Factory, (&admin, wasm_hashes, fee_config)));
        let market = factory.create_market(
            &admin,
            &vault,
            &VaultType::Vault4626,
            &maturity,
            &CURRENT_APY,
            &APY_MIN,
            &APY_MAX,
            &FEE_APY,
        );

        let f = ZapFixture {
            env: env.clone(),
            admin,
            user,
            asset,
            vault,
            ym: market.ym.clone(),
            pt: market.pt.clone(),
            yt: market.yt.clone(),
            pool: LiquidityPoolClient::new(env, &market.pool),
            router: RouterClient::new(env, &env.register(RouterContract, (&factory.address,))),
            maturity,
        };

        // Seed the pool the long way round (deposit → split → LP) rather than by
        // minting shares, so every share in the system is backed by a real asset
        // and a redeem can actually settle.
        StandardVaultClient::new(env, &f.vault).deposit(&4_000_000_000, &f.user, &f.user, &f.user);

        f.approve(&f.vault, &f.ym, 2_000_000_000);
        env.invoke_contract::<()>(
            &f.ym,
            &Symbol::new(env, "deposit"),
            (f.user.clone(), 2_000_000_000i128).into_val(env),
        );

        f.approve(&f.pt, &f.pool.address, POOL_PT);
        f.approve(&f.vault, &f.pool.address, POOL_V);
        f.pool.deposit(&f.user, &POOL_PT, &0, &POOL_V, &0);

        f
    }

    /// A second (or third) funded participant, holding only the base asset —
    /// exactly the position a new user arrives in.
    pub fn add_actor(&self) -> Address {
        let actor = Address::generate(&self.env);
        token::StellarAssetClient::new(&self.env, &self.asset).mint(&actor, &USER_ASSET);
        actor
    }

    pub fn approve(&self, token_addr: &Address, spender: &Address, amount: i128) {
        self.approve_for(&self.user, token_addr, spender, amount);
    }

    pub fn approve_for(&self, who: &Address, token_addr: &Address, spender: &Address, amount: i128) {
        token::TokenClient::new(&self.env, token_addr).approve(
            who,
            spender,
            &amount,
            &(self.env.ledger().sequence() + 1000),
        );
    }

    pub fn balance(&self, token_addr: &Address) -> i128 {
        self.balance_of(token_addr, &self.user)
    }

    pub fn balance_of(&self, token_addr: &Address, who: &Address) -> i128 {
        token::TokenClient::new(&self.env, token_addr).balance(who)
    }

    pub fn total_supply(&self, token_addr: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            token_addr,
            &Symbol::new(&self.env, "total_supply"),
            soroban_sdk::Vec::new(&self.env),
        )
    }

    /// Simulates yield the only way a real vault can produce it: more underlying
    /// arrives, so each share is worth more. There is no rate setter to cheat
    /// with here, which is the point of using this vault.
    pub fn accrue_yield(&self, amount: i128) {
        token::StellarAssetClient::new(&self.env, &self.asset).mint(&self.vault, &amount);
    }

    pub fn advance_time(&self, secs: u64) {
        self.env.ledger().with_mut(|l| l.timestamp += secs);
    }

    pub fn advance_past_maturity(&self) {
        self.env
            .ledger()
            .with_mut(|l| l.timestamp = self.maturity + 1);
    }
}
