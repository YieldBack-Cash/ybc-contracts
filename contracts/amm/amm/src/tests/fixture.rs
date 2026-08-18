use soroban_sdk::{Address, Env, String};
use soroban_sdk::testutils::{Address as _, Ledger};

use crate::contract::{LiquidityPool, LiquidityPoolClient};
use mock_vault::MockVaultClient;
use yield_manager::{VaultType, YieldManager};

// Default market params (all 1e7-scaled APYs)
pub const CURRENT_APY: i128 = 1_000_000; // 10% — opening implied rate
pub const APY_MIN: i128 = 200_000;       // 2%  — bottom of the trading band
pub const APY_MAX: i128 = 2_000_000;     // 20% — top of the trading band
pub const FEE_APY: i128 = 100_000;       // 1%  — fee as an annualized rate spread

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

pub struct AmmFixture<'a> {
    pub env: Env,
    /// PT token client (mock_vault registered first → lower counter address).
    pub pt: MockVaultClient<'a>,
    pub vault: MockVaultClient<'a>,
    pub pool: LiquidityPoolClient<'a>,
    pub admin: Address,
    pub user: Address,
    /// The real yield manager, pointed at `vault`. The pool reads its exchange
    /// rate from here on every swap, so this is a live `YieldManager` and not a
    /// stand-in — a stub would be free to answer with the vault's rate and hide
    /// the divergence that PT pricing depends on.
    ///
    /// Flash tests replace it, via `env.register_at`, with a receiver that can
    /// misbehave on purpose; the real YM cannot under-repay, which is the whole
    /// point of those tests.
    pub ym: Address,
    /// Protocol fee sink baked into the pool at construction.
    pub treasury: Address,
}

impl<'a> AmmFixture<'a> {
    /// Deploy env, mock PT, mock vault, and the AMM pool with the reserve fee
    /// off (rate 0) — most tests assert exact amounts under fee-free reserves.
    /// Expiry is set to `now + ONE_YEAR_SECS` by default.
    pub fn new(env: &'a Env) -> Self {
        Self::new_with_reserve_fee(env, 0)
    }

    /// Same as `new`, but with a nonzero treasury cut of the trading fee
    /// (1e7-scaled fraction, e.g. 1_000_000 = 10% of the fee).
    ///
    /// Both tokens are registered via `env.register` so their addresses come
    /// from the same sequential counter.  PT is registered first, guaranteeing
    /// `pt_addr < vault_addr` without any randomness.
    pub fn new_with_reserve_fee(env: &'a Env, reserve_fee_rate: i128) -> Self {
        env.ledger().with_mut(|l| { l.timestamp = 1_000_000; });

        let admin = Address::generate(env);
        let user  = Address::generate(env);

        let pt_addr = env.register(
            mock_vault::MockVault,
            (admin.clone(), String::from_str(env, "Principal Token"), String::from_str(env, "PT"), 7u32),
        );
        let pt = MockVaultClient::new(env, &pt_addr);

        let vault_addr = env.register(
            mock_vault::MockVault,
            (admin.clone(), String::from_str(env, "Vault"), String::from_str(env, "VLT"), 7u32),
        );
        let vault = MockVaultClient::new(env, &vault_addr);

        assert!(pt_addr < vault_addr, "counter addresses must be sequential");

        vault.set_exchange_rate(&1_000_0000);

        let treasury = Address::generate(env);
        let now    = env.ledger().timestamp();
        let expiry = now + ONE_YEAR_SECS;

        // The real yield manager, sharing the pool's expiry so its rate locks in
        // step with the market. It needs no PT/YT wiring for these tests: the pool
        // only calls `get_exchange_rate`, which the constructor alone satisfies.
        let ym = env.register(
            YieldManager,
            (admin.clone(), vault_addr.clone(), VaultType::Vault4626, expiry, treasury.clone()),
        );

        let pool_addr = env.register(
            LiquidityPool,
            (pt_addr.clone(), vault_addr.clone(), expiry, CURRENT_APY, APY_MIN, APY_MAX, FEE_APY, ym.clone(), treasury.clone(), reserve_fee_rate),
        );
        let pool = LiquidityPoolClient::new(env, &pool_addr);

        pt.mint(&admin, &1_000_000_000);
        pt.mint(&user,  &1_000_000_000);
        vault.mint(&admin, &1_000_000_000);
        vault.mint(&user,  &1_000_000_000);

        AmmFixture { env: env.clone(), pt, vault, pool, admin, user, ym, treasury }
    }

    pub fn set_time(&self, ts: u64) {
        self.env.ledger().with_mut(|l| l.timestamp = ts);
    }

    pub fn set_vault_rate(&self, rate: i128) {
        self.vault.set_exchange_rate(&rate);
    }

    /// Approve and deposit into the pool.
    pub fn deposit(&self, from: &Address, pt_amount: i128, v_amount: i128) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.pt.approve(from, &self.pool.address, &pt_amount, &expiry_ledger);
        self.vault.approve(from, &self.pool.address, &v_amount, &expiry_ledger);
        self.pool.deposit(from, &pt_amount, &0, &v_amount, &0);
    }

    /// Approve and sell an exact amount of PT into the pool for V.
    pub fn swap_pt_for_v(&self, from: &Address, pt_in: i128, min_v_out: i128) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.pt.approve(from, &self.pool.address, &pt_in, &expiry_ledger);
        self.pool.swap_pt_for_v(from, &pt_in, &min_v_out);
    }

    /// Approve and buy an exact amount of PT out of the pool, paying V.
    pub fn swap_v_for_pt(&self, from: &Address, pt_out: i128, v_in_max: i128) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.vault.approve(from, &self.pool.address, &v_in_max, &expiry_ledger);
        self.pool.swap_v_for_pt(from, &pt_out, &v_in_max);
    }

    /// Total LP shares outstanding = the sole depositor's balance plus the dead burn.
    /// Valid only in tests where `admin` is the only non-burn shareholder.
    pub fn total_shares_admin_only(&self) -> i128 {
        self.pool.balance_shares(&self.admin) + 100
    }
}