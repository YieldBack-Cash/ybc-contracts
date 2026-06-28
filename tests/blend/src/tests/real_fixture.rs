use soroban_sdk::{
    contracttype, token::StellarAssetClient, Address, BytesN, Env, String, Symbol,
};
use soroban_sdk::testutils::{Address as _, BytesN as _, Ledger};

use blend_contract_sdk::{
    pool::{Client as BlendPoolClient, Request},
    testutils::{BlendFixture, default_reserve_config},
};

use principal_token::PrincipalToken;
use yield_manager::{YieldManager, VaultType};
use yield_manager_interface::YieldManagerClient;
use yield_token::YieldToken;

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;
pub const ONE_DAY_SECS: u64 = 24 * 3600;

mod fee_vault {
    soroban_sdk::contractimport!(file = "../../wasms/fee_vault_v2.wasm");
}

// SEP-40 compatible types — XDR-equivalent to sep-40-oracle's Asset and PriceData.
// Soroban encodes contracttype enums/structs by field name, so these are wire-compatible
// with any contract compiled against sep-40-oracle as long as names and types match.
#[contracttype]
#[derive(Clone)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

// ── Inline mock SEP-40 price oracle ──────────────────────────────────────────
mod mock_oracle {
    use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};
    use super::{Asset, PriceData};

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        /// Set the USD price for an asset (7-decimal precision, e.g. $1.00 = 1_000_0000).
        pub fn set_price(e: Env, asset: Address, price: i128) {
            e.storage().persistent().set(&asset, &price);
        }

        pub fn base(e: Env) -> Asset {
            Asset::Other(Symbol::new(&e, "USD"))
        }

        pub fn decimals(_e: Env) -> u32 {
            7
        }

        pub fn resolution(_e: Env) -> u32 {
            300
        }

        pub fn lastprice(e: Env, asset: Asset) -> Option<PriceData> {
            if let Asset::Stellar(addr) = asset {
                let price: Option<i128> = e.storage().persistent().get(&addr);
                price.map(|p| PriceData {
                    price: p,
                    timestamp: e.ledger().timestamp(),
                })
            } else {
                None
            }
        }

        pub fn price(e: Env, asset: Asset, _timestamp: u64) -> Option<PriceData> {
            Self::lastprice(e, asset)
        }

        pub fn prices(e: Env, asset: Asset, _records: u32) -> Option<Vec<PriceData>> {
            let data = Self::lastprice(e.clone(), asset)?;
            let mut v = Vec::new(&e);
            v.push_back(data);
            Some(v)
        }
    }
}

// ── Fixture ───────────────────────────────────────────────────────────────────

pub struct RealBlendFixture<'a> {
    pub env: Env,
    pub admin: Address,
    pub user: Address,
    pub underlying: Address,
    pub fee_vault: fee_vault::Client<'a>,
    pub blend_pool: BlendPoolClient<'a>,
    pub yield_manager: Address,
    pub pt: Address,
    pub yt: Address,
    pub maturity: u64,
}

impl<'a> RealBlendFixture<'a> {
    /// Deploy the full stack:
    ///
    /// Blend protocol (BlendFixture) → real pool with seeded utilisation
    ///   → fee vault (fee_vault_v2.wasm) → yield manager → PT / YT
    ///
    /// The fee vault is bootstrapped with a tiny admin deposit so its
    /// `total_shares > 0` before the YM constructor calls `convert_to_assets`.
    pub fn new(env: &'a Env) -> Self {
        env.mock_all_auths_allowing_non_root_auth();
        env.ledger().with_mut(|l| {
            l.timestamp = 1_000_000;
            l.sequence_number = 100;
        });
        env.cost_estimate().budget().reset_unlimited();

        let admin = Address::generate(env);
        let user = Address::generate(env);
        let maturity = env.ledger().timestamp() + ONE_YEAR_SECS;

        // ── BLND + USDC — required by BlendFixture::deploy ──────────────────
        let blnd = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let usdc = env.register_stellar_asset_contract_v2(admin.clone()).address();

        // ── Blend protocol stack ─────────────────────────────────────────────
        let blend = BlendFixture::deploy(env, &admin, &blnd, &usdc);

        // ── Underlying asset ─────────────────────────────────────────────────
        let underlying = env.register_stellar_asset_contract_v2(admin.clone()).address();
        StellarAssetClient::new(env, &underlying).mint(&admin, &20_000_000_0000000i128);

        // ── Mock oracle — $1.00 per underlying token ─────────────────────────
        let oracle = env.register(mock_oracle::MockOracle {}, ());
        mock_oracle::MockOracleClient::new(env, &oracle)
            .set_price(&underlying, &1_000_0000i128);

        // ── Pool ─────────────────────────────────────────────────────────────
        let pool_addr = blend.pool_factory.deploy(
            &admin,
            &String::from_str(env, "YBC"),
            &BytesN::random(env),
            &oracle,
            &0u32,          // backstop take rate (0%)
            &4u32,          // max positions
            &1_0000000i128, // min collateral ($1)
        );
        let blend_pool = BlendPoolClient::new(env, &pool_addr);

        // ── Reserve ───────────────────────────────────────────────────────────
        blend_pool.queue_set_reserve(&underlying, &default_reserve_config());
        blend_pool.set_reserve(&underlying);

        // ── Backstop deposit + pool activation ───────────────────────────────
        blend.backstop.deposit(&admin, &pool_addr, &50_000_0000000i128);
        blend_pool.set_status(&3u32);
        blend_pool.update_status();

        // ── Seed utilisation so b_rate accrues over time ─────────────────────
        // Admin supplies collateral then borrows at ~50% utilisation.
        let mut seed_requests = soroban_sdk::Vec::new(env);
        seed_requests.push_back(Request {
            address: underlying.clone(),
            amount: 10_000_000_0000000i128,
            request_type: 2, // supply as collateral
        });
        seed_requests.push_back(Request {
            address: underlying.clone(),
            amount: 5_000_000_0000000i128,
            request_type: 4, // borrow
        });
        blend_pool.submit(&admin, &admin, &admin, &seed_requests);

        // ── Fee vault ────────────────────────────────────────────────────────
        let fee_vault_addr = env.register(
            fee_vault::WASM,
            (&admin, &pool_addr, &underlying, Option::<Address>::None),
        );
        let fee_vault_client = fee_vault::Client::new(env, &fee_vault_addr);

        // Bootstrap: deposit 1 token so total_shares > 0 before the YM
        // constructor calls fee_vault.convert_to_assets (which divides by total_shares).
        fee_vault_client.deposit(&1_0000000i128, &admin, &admin, &admin);

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

        YieldManagerClient::new(env, &ym_addr).set_token_contracts(&pt_addr, &yt_addr);

        RealBlendFixture {
            env: env.clone(),
            admin,
            user,
            underlying,
            fee_vault: fee_vault_client,
            blend_pool,
            yield_manager: ym_addr,
            pt: pt_addr,
            yt: yt_addr,
            maturity,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Mint underlying tokens to `to`.
    pub fn mint_underlying(&self, to: &Address, amount: i128) {
        StellarAssetClient::new(&self.env, &self.underlying).mint(to, &amount);
    }

    /// Full deposit path: underlying → fee vault shares → YM → PT + YT.
    pub fn setup_yt_position(&self, user: &Address, underlying_amount: i128) {
        self.mint_underlying(user, underlying_amount);
        let shares = self.fee_vault.deposit(&underlying_amount, user, user, user);
        let expiry = self.env.ledger().sequence() + 1000;
        self.fee_vault.approve(user, &self.yield_manager, &shares, &expiry);
        YieldManagerClient::new(&self.env, &self.yield_manager).deposit(user, &shares);
    }

    /// Advance timestamp and ledger sequence (Stellar: ~5 s/ledger).
    pub fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|l| {
            l.timestamp += seconds;
            l.sequence_number += (seconds / 5).max(1) as u32;
        });
    }

    /// Trigger pool interest accrual. Blend only updates b_rate in storage
    /// when `submit` is called, so tests must call this after advancing time
    /// before the fee vault's convert_to_assets will see the new rate.
    pub fn accrue_interest(&self) {
        // Repay a small amount — any pool interaction triggers accrual.
        let mut requests = soroban_sdk::Vec::new(&self.env);
        requests.push_back(Request {
            address: self.underlying.clone(),
            amount: 1_000_000i128,
            request_type: 5, // repay
        });
        self.blend_pool.submit(&self.admin, &self.admin, &self.admin, &requests);
    }

    pub fn claim_yield(&self, user: &Address) -> i128 {
        use yield_token_interface::YieldTokenClient;
        YieldTokenClient::new(&self.env, &self.yt).claim_yield(user)
    }

    pub fn pt_balance(&self, user: &Address) -> i128 {
        soroban_sdk::token::Client::new(&self.env, &self.pt).balance(user)
    }

    pub fn yt_balance(&self, user: &Address) -> i128 {
        soroban_sdk::token::Client::new(&self.env, &self.yt).balance(user)
    }

    pub fn vault_shares(&self, user: &Address) -> i128 {
        self.fee_vault.get_shares(user)
    }
}