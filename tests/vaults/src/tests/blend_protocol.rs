//! A real Blend deployment from the binaries vendored in `wasms/blend/` (see
//! the `MANIFEST.md` there). The same approach `ybc-vaults` takes, for the
//! same reason: `blend-contract-sdk` pins its own `soroban-sdk`.

use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, BytesN as _},
    token::StellarAssetClient,
    vec, Address, BytesN, Env, String, Symbol, Vec,
};

pub mod pool {
    soroban_sdk::contractimport!(file = "../../wasms/blend/pool.wasm");
}
pub mod pool_factory {
    soroban_sdk::contractimport!(file = "../../wasms/blend/pool_factory.wasm");
}
pub mod backstop {
    soroban_sdk::contractimport!(file = "../../wasms/blend/backstop.wasm");
}
pub mod emitter {
    soroban_sdk::contractimport!(file = "../../wasms/blend/emitter.wasm");
}
pub mod comet {
    soroban_sdk::contractimport!(file = "../../wasms/blend/comet.wasm");
}

// ── SEP-40 oracle ────────────────────────────────────────────────────────────

/// XDR-equivalent to the types the pool expects: Soroban encodes by field and
/// variant name, so these are wire-compatible as long as the names match.
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

#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    /// Set the USD price for an asset (7 decimals, e.g. $1.00 = 1_000_0000).
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

// ── protocol ─────────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub struct BlendProtocol<'a> {
    pub backstop: backstop::Client<'a>,
    pub emitter: emitter::Client<'a>,
    pub pool_factory: pool_factory::Client<'a>,
}

/// Deploys emitter, Comet backstop token, backstop and pool factory. Mints
/// 200k backstop tokens to `deployer`. A port of
/// `blend_contract_sdk::testutils::BlendFixture::deploy`.
pub fn deploy<'a>(env: &Env, deployer: &Address, blnd: &Address, usdc: &Address) -> BlendProtocol<'a> {
    let emitter = env.register(emitter::WASM, ());
    let backstop = Address::generate(env);
    let pool_factory = Address::generate(env);
    let comet = env.register(comet::WASM, ());
    let blnd_client = StellarAssetClient::new(env, blnd);
    let usdc_client = StellarAssetClient::new(env, usdc);
    blnd_client.mint(deployer, &(1_000_0000000 * 2001));
    usdc_client.mint(deployer, &(25_0000000 * 2001));

    let comet_client = comet::Client::new(env, &comet);
    comet_client.init(
        deployer,
        &vec![env, blnd.clone(), usdc.clone()],
        &vec![env, 0_8000000, 0_2000000],
        &vec![env, 1_000_0000000, 25_0000000],
        &0_0030000,
    );
    comet_client.join_pool(
        &199_900_0000000, // finalize mints 100
        &vec![env, 1_000_0000000 * 2000, 25_0000000 * 2000],
        deployer,
    );

    blnd_client.set_admin(&emitter);
    let emitter_client = emitter::Client::new(env, &emitter);
    emitter_client.initialize(blnd, &backstop, &comet);

    env.register_at(
        &backstop,
        backstop::WASM,
        (
            comet,
            emitter,
            blnd,
            usdc,
            pool_factory.clone(),
            Vec::<(Address, i128)>::new(env),
        ),
    );
    let backstop_client = backstop::Client::new(env, &backstop);

    let pool_hash = env.deployer().upload_contract_wasm(pool::WASM);
    env.register_at(
        &pool_factory,
        pool_factory::WASM,
        (pool_factory::PoolInitMeta {
            backstop,
            blnd_id: blnd.clone(),
            pool_hash,
        },),
    );

    BlendProtocol {
        backstop: backstop_client,
        emitter: emitter_client,
        pool_factory: pool_factory::Client::new(env, &pool_factory),
    }
}

/// The SDK's "good enough" reserve config.
pub fn default_reserve_config() -> pool::ReserveConfig {
    pool::ReserveConfig {
        decimals: 7,
        c_factor: 0_7500000,
        l_factor: 0_7500000,
        util: 0_7500000,
        max_util: 0_9500000,
        r_base: 0_0100000,
        r_one: 0_0500000,
        r_two: 0_5000000,
        r_three: 1_5000000,
        reactivity: 0_0000020,
        index: 0,
        supply_cap: 100_000_000_0000000,
        enabled: true,
    }
}

/// Deploys a pool with one reserve (`underlying`, priced at $1), activates it,
/// and seeds ~50% utilisation so `b_rate` accrues over time. `admin` must
/// already hold enough `underlying` for the seed (20M at 7 decimals).
pub fn deploy_pool(env: &Env, protocol: &BlendProtocol, admin: &Address, underlying: &Address) -> Address {
    let oracle = env.register(MockOracle {}, ());
    MockOracleClient::new(env, &oracle).set_price(underlying, &1_000_0000);

    let pool_addr = protocol.pool_factory.deploy(
        admin,
        &String::from_str(env, "YBC"),
        &BytesN::random(env),
        &oracle,
        &0u32,          // backstop take rate (0%)
        &4u32,          // max positions
        &1_0000000i128, // min collateral ($1)
    );
    let blend_pool = pool::Client::new(env, &pool_addr);

    blend_pool.queue_set_reserve(underlying, &default_reserve_config());
    blend_pool.set_reserve(underlying);

    protocol.backstop.deposit(admin, &pool_addr, &50_000_0000000i128);
    blend_pool.set_status(&3u32);
    blend_pool.update_status();

    // Admin supplies collateral then borrows at ~50% utilisation.
    blend_pool.submit(
        admin,
        admin,
        admin,
        &vec![
            env,
            pool::Request {
                address: underlying.clone(),
                amount: 10_000_000_0000000i128,
                request_type: 2, // supply as collateral
            },
            pool::Request {
                address: underlying.clone(),
                amount: 5_000_000_0000000i128,
                request_type: 4, // borrow
            },
        ],
    );

    pool_addr
}
