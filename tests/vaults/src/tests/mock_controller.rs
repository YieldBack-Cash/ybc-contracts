//! A stand-in for the XOXNO lending controller, copied from
//! `ybc-vaults/crates/xoxno-vault/src/testutils.rs` so this workspace needs no
//! path dependency on that one. XOXNO publishes no SDK or test fixture; this
//! is the mock the adapter's own suite runs on.
//!
//! Three behaviours are reproduced deliberately, because the adapter's
//! correctness depends on them: positions are stored as 27-decimal `Ray`
//! values (`from_asset(amount) / index`, so 10 XLM at index RAY is 1e28, not
//! 1e8; verified on the testnet controller), a supply credits `floor(...)`
//! scaled units while a withdrawal burns `ceil(...)`, and a withdrawal amount
//! of `0` means *take everything*.
//!
//! The types are declared by name to match the deployed controller's; Soroban
//! encodes `contracttype` values by field name, so a rename fails at the call
//! boundary rather than at compile time.

use soroban_sdk::{
    contract, contractimpl, contracttype, token::TokenClient, Address, Env, Map, Vec, U256,
};

/// Interest indexes are 27-decimal fixed point.
pub const RAY: i128 = 1_000_000_000_000_000_000_000_000_000;

pub const HUB_ID: u32 = 1;
pub const SPOKE_ID: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubAssetKey {
    pub asset: Address,
    pub hub_id: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketIndexRaw {
    pub borrow_index: i128,
    pub supply_index: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountPositionRaw {
    pub liquidation_bonus: u32,
    pub liquidation_fees: u32,
    pub liquidation_threshold: u32,
    pub loan_to_value: u32,
    pub scaled_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebtPositionRaw {
    pub scaled_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpokeAssetConfig {
    pub borrow_cap: i128,
    pub frozen: bool,
    pub is_borrowable: bool,
    pub is_collateralizable: bool,
    pub liquidation_bonus: u32,
    pub liquidation_fees: u32,
    pub liquidation_threshold: u32,
    pub loan_to_value: u32,
    pub no_seize: bool,
    pub paused: bool,
    pub supply_cap: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpokeUsageRaw {
    pub borrowed_scaled_ray: i128,
    pub supplied_scaled_ray: i128,
}

/// Spoke 1's live mainnet supply cap for hub 1 USDC: 5,000,000 at 7 decimals.
pub const DEFAULT_SUPPLY_CAP: i128 = 50_000_000_000_000;

fn mul_div_floor(e: &Env, x: i128, y: i128, denominator: i128) -> i128 {
    if x == 0 || y == 0 {
        return 0;
    }
    let n = U256::from_u128(e, x as u128).mul(&U256::from_u128(e, y as u128));
    n.div(&U256::from_u128(e, denominator as u128))
        .to_u128()
        .unwrap() as i128
}

/// `10^(27 - 7)`: the factor between a 7-decimal asset amount and its Ray form.
const SCALED_UNIT: i128 = 100_000_000_000_000_000_000;

/// `Ray::from_asset(amount, 7).div_floor(index)`.
fn ray_scaled_floor(e: &Env, amount: i128, index: i128) -> i128 {
    mul_div_floor(e, amount * SCALED_UNIT, RAY, index)
}

fn ray_scaled_ceil(e: &Env, amount: i128, index: i128) -> i128 {
    let floor = ray_scaled_floor(e, amount, index);
    if ray_to_assets(e, floor, index) < amount {
        floor + 1
    } else {
        floor
    }
}

/// `scaled * index / RAY`, back at 7 decimals.
fn ray_to_assets(e: &Env, scaled: i128, index: i128) -> i128 {
    mul_div_floor(e, scaled, index, RAY) / SCALED_UNIT
}

#[contracttype]
pub enum MockKey {
    Index,
    NextId,
    HubAsset,
    Scaled(u64),
    Spoke(u64),
    TotalScaled,
    SupplyCap,
    Paused,
}

#[contract]
pub struct MockController;

#[contractimpl]
impl MockController {
    pub fn __constructor(e: Env, asset: Address, hub_id: u32) {
        e.storage().instance().set(&MockKey::Index, &RAY);
        e.storage().instance().set(&MockKey::NextId, &1u64);
        e.storage()
            .instance()
            .set(&MockKey::HubAsset, &HubAssetKey { asset, hub_id });
        e.storage()
            .instance()
            .set(&MockKey::SupplyCap, &DEFAULT_SUPPLY_CAP);
        e.storage().instance().set(&MockKey::Paused, &false);
    }

    // ── test controls ───────────────────────────────────────────────────────

    /// Move the market's supply index. Raising it simulates interest accrual;
    /// lowering it simulates `seize_positions` socializing bad debt.
    pub fn set_supply_index(e: Env, index: i128) {
        e.storage().instance().set(&MockKey::Index, &index);
    }

    pub fn scaled(e: Env, account_id: u64) -> i128 {
        e.storage()
            .instance()
            .get(&MockKey::Scaled(account_id))
            .unwrap_or(0)
    }

    // ── controller surface ──────────────────────────────────────────────────

    pub fn supply(
        e: Env,
        caller: Address,
        account_id: u64,
        spoke_id: u32,
        assets: Vec<(HubAssetKey, i128)>,
    ) -> u64 {
        let (key, amount) = assets.get(0).expect("empty payment vector");
        let index = Self::index(e.clone());

        // The vault pre-authorized exactly this transfer before calling.
        TokenClient::new(&e, &key.asset).transfer(&caller, &e.current_contract_address(), &amount);

        let id = if account_id == 0 {
            let next: u64 = e.storage().instance().get(&MockKey::NextId).unwrap_or(1);
            e.storage().instance().set(&MockKey::NextId, &(next + 1));
            e.storage().instance().set(&MockKey::Spoke(next), &spoke_id);
            next
        } else {
            account_id
        };

        // Credit floors: the depositor never gains from rounding.
        let delta = ray_scaled_floor(&e, amount, index);
        let scaled = Self::scaled(e.clone(), id) + delta;
        e.storage().instance().set(&MockKey::Scaled(id), &scaled);
        Self::bump_total(&e, delta);
        id
    }

    pub fn withdraw(
        e: Env,
        caller: Address,
        account_id: u64,
        withdrawals: Vec<(HubAssetKey, i128)>,
        to: Option<Address>,
    ) -> Vec<(HubAssetKey, i128)> {
        let (key, requested) = withdrawals.get(0).expect("empty withdrawal vector");
        let index = Self::index(e.clone());
        let scaled = Self::scaled(e.clone(), account_id);

        // A requested amount of zero means "everything in this market".
        let amount = if requested == 0 {
            ray_to_assets(&e, scaled, index)
        } else {
            requested
        };

        // The burn ceils: the protocol never loses to rounding.
        let burn = ray_scaled_ceil(&e, amount, index);
        assert!(burn <= scaled, "withdraw exceeds position");

        e.storage()
            .instance()
            .set(&MockKey::Scaled(account_id), &(scaled - burn));
        Self::bump_total(&e, -burn);

        let recipient = to.unwrap_or(caller);
        TokenClient::new(&e, &key.asset).transfer(&e.current_contract_address(), &recipient, &amount);

        soroban_sdk::vec![&e, (key, amount)]
    }

    pub fn get_collateral_amount(e: Env, account_id: u64, _hub_asset: HubAssetKey) -> i128 {
        let index = Self::index(e.clone());
        ray_to_assets(&e, Self::scaled(e.clone(), account_id), index)
    }

    pub fn get_market_index(e: Env, _hub_asset: HubAssetKey) -> MarketIndexRaw {
        MarketIndexRaw {
            borrow_index: RAY,
            supply_index: Self::index(e),
        }
    }

    pub fn get_account_positions(
        e: Env,
        account_id: u64,
    ) -> (
        Map<HubAssetKey, AccountPositionRaw>,
        Map<HubAssetKey, DebtPositionRaw>,
    ) {
        let key = Self::hub_asset(e.clone());
        let mut supplies = Map::new(&e);
        supplies.set(
            key,
            AccountPositionRaw {
                liquidation_bonus: 400,
                liquidation_fees: 1000,
                liquidation_threshold: 8000,
                loan_to_value: 7600,
                scaled_amount: Self::scaled(e.clone(), account_id),
            },
        );
        (supplies, Map::new(&e))
    }

    pub fn account_exists(e: Env, account_id: u64) -> bool {
        let next: u64 = e.storage().instance().get(&MockKey::NextId).unwrap_or(1);
        account_id != 0 && account_id < next
    }

    /// The mock is its own pool: it holds the cash as well as the accounting.
    pub fn get_pool_address(e: Env) -> Address {
        e.current_contract_address()
    }

    pub fn get_spoke_asset(e: Env, _spoke_id: u32, _hub_asset: HubAssetKey) -> SpokeAssetConfig {
        SpokeAssetConfig {
            borrow_cap: 37_500_000_000_000,
            frozen: false,
            is_borrowable: true,
            is_collateralizable: true,
            liquidation_bonus: 400,
            liquidation_fees: 1000,
            liquidation_threshold: 8000,
            loan_to_value: 7600,
            no_seize: false,
            paused: e.storage().instance().get(&MockKey::Paused).unwrap_or(false),
            supply_cap: e
                .storage()
                .instance()
                .get(&MockKey::SupplyCap)
                .unwrap_or(DEFAULT_SUPPLY_CAP),
        }
    }

    pub fn get_spoke_usage(e: Env, _spoke_id: u32, _hub_asset: HubAssetKey) -> SpokeUsageRaw {
        SpokeUsageRaw {
            borrowed_scaled_ray: 0,
            supplied_scaled_ray: Self::total_scaled(e),
        }
    }

    pub fn total_scaled(e: Env) -> i128 {
        e.storage()
            .instance()
            .get(&MockKey::TotalScaled)
            .unwrap_or(0)
    }

    // ── internals ───────────────────────────────────────────────────────────

    fn bump_total(e: &Env, delta: i128) {
        let total = Self::total_scaled(e.clone()) + delta;
        e.storage().instance().set(&MockKey::TotalScaled, &total);
    }

    fn index(e: Env) -> i128 {
        e.storage().instance().get(&MockKey::Index).unwrap_or(RAY)
    }

    fn hub_asset(e: Env) -> HubAssetKey {
        e.storage()
            .instance()
            .get(&MockKey::HubAsset)
            .expect("mock not constructed")
    }
}
