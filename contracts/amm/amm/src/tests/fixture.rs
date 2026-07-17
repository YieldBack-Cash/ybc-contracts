use soroban_sdk::{Address, Env, String};
use soroban_sdk::testutils::{Address as _, Ledger};

use crate::contract::{LiquidityPool, LiquidityPoolClient};
use mock_vault::MockVaultClient;

// ── Constants ────────────────────────────────────────────────────────────────

// Default market params (all 1e7-scaled)
pub const SCALAR_ROOT: i128 = 250_000_000;   // 25.0 — moderate curve steepness
pub const FEE_RATE_ROOT: i128 = 500_000;     // 0.05 — 5% annualised fee root
pub const INITIAL_ANCHOR: i128 = 11_000_000; // 1.1  — 10% initial implied rate anchor
pub const LAST_IMPLIED_RATE: i128 = 1_000_000; // 0.1 — 10% starting implied rate

pub const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

// ── Fixture ──────────────────────────────────────────────────────────────────

pub struct AmmFixture<'a> {
    pub env: Env,
    /// PT token client (mock_vault registered first → lower counter address).
    pub pt: MockVaultClient<'a>,
    /// Vault (V) token client.
    pub vault: MockVaultClient<'a>,
    pub pool: LiquidityPoolClient<'a>,
    pub admin: Address,
    pub user: Address,
    /// The pool's trusted flash-swap receiver. Flash tests deploy their mock
    /// receiver at this address via `env.register_at` so it is accepted by the pool.
    pub ym: Address,
}

impl<'a> AmmFixture<'a> {
    /// Deploy env, mock PT, mock vault, and the AMM pool.
    /// Expiry is set to `now + ONE_YEAR_SECS` by default.
    ///
    /// Both tokens are registered via `env.register` so their addresses come
    /// from the same sequential counter.  PT is registered first, guaranteeing
    /// `pt_addr < vault_addr` without any randomness.
    pub fn new(env: &'a Env) -> Self {
        env.ledger().with_mut(|l| { l.timestamp = 1_000_000; });

        let admin = Address::generate(env);
        let user  = Address::generate(env);

        // PT registered first → lower counter address.
        let pt_addr = env.register(
            mock_vault::MockVault,
            (admin.clone(), String::from_str(env, "Principal Token"), String::from_str(env, "PT"), 7u32),
        );
        let pt = MockVaultClient::new(env, &pt_addr);

        // Vault registered second → higher counter address.
        let vault_addr = env.register(
            mock_vault::MockVault,
            (admin.clone(), String::from_str(env, "Vault"), String::from_str(env, "VLT"), 7u32),
        );
        let vault = MockVaultClient::new(env, &vault_addr);

        assert!(pt_addr < vault_addr, "counter addresses must be sequential");

        // Trusted flash-swap receiver. Held as a plain address here; flash tests
        // deploy a mock receiver at this address (via `env.register_at`) so the
        // pool accepts it, while other tests never touch the flash entrypoints.
        let ym = Address::generate(env);

        // AMM pool.
        let now    = env.ledger().timestamp();
        let expiry = now + ONE_YEAR_SECS;
        let pool_addr = env.register(
            LiquidityPool,
            (pt_addr.clone(), vault_addr.clone(), expiry, SCALAR_ROOT, INITIAL_ANCHOR, FEE_RATE_ROOT, LAST_IMPLIED_RATE, ym.clone()),
        );
        let pool = LiquidityPoolClient::new(env, &pool_addr);

        vault.set_exchange_rate(&1_000_0000);

        // Fund admin and user.
        pt.mint(&admin, &1_000_000_000);
        pt.mint(&user,  &1_000_000_000);
        vault.mint(&admin, &1_000_000_000);
        vault.mint(&user,  &1_000_000_000);

        AmmFixture { env: env.clone(), pt, vault, pool, admin, user, ym }
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