#![no_std]

use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "AmmClient")]
pub trait AmmInterface {
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128);
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128);
    fn flash_swap_pt(env: Env, receiver: Address, yt_out: i128, user: Address, max_v_in: i128);
    fn flash_swap_v(env: Env, receiver: Address, pt_to_borrow: i128, user: Address, min_v_out: i128);
    fn deposit(env: Env, to: Address, desired_a: i128, min_a: i128, desired_b: i128, min_b: i128);
    fn withdraw(env: Env, to: Address, share_amount: i128, min_a: i128, min_b: i128) -> (i128, i128);
    fn get_reserves(env: Env) -> (i128, i128);
    fn get_implied_rate(env: Env) -> i128;
    fn get_treasury(env: Env) -> Address;
    fn get_reserve_fee_rate(env: Env) -> i128;
    fn balance_shares(env: Env, user: Address) -> i128;
    fn get_total_shares(env: Env) -> i128;
}

// `vault_rate` on both callbacks below is the vault's share/asset rate — assets per
// 1e7 shares, exactly what `convert_to_assets(1e7)` returns — read by the AMM at the
// top of the flash swap for its own pricing and handed down rather than re-fetched.
//
// It exists purely to remove a redundant read. Against a lending vault the call is
// expensive (it accrues interest and materialises the pool's whole reserve record),
// and the rate cannot move mid-transaction, so the receiver asking again would spend
// that cost to recompute a value the caller is already holding.
//
// The direction matters. The value flows from the contract that read the vault
// DIRECTLY to the contract that applies policy on top of it, never the reverse: a
// receiver must not source its rate from a caller that itself holds only a derived
// or high-water-marked figure. Receivers should still treat it as an input to
// validate, not a fact — the yield manager, for instance, keeps its own
// non-decreasing floor, so a too-low value is absorbed and only the registered pool
// can supply one at all.

#[contractclient(name = "FlashSwapPtReceiverClient")]
pub trait FlashSwapPtReceiver {
    /// Called by the AMM during `flash_swap_pt` (buy YT). The pool has already advanced
    /// `v_from_pool` vault shares to the receiver as payment for the PT it is buying. The
    /// receiver must mint `yt_out` (PT + YT) using that V plus the user's top-up, deliver
    /// `yt_out` YT to `user`, and return exactly `yt_out` PT to `amm` before returning.
    /// The user's total V cost must not exceed `max_v_in`.
    fn on_flash_receive_pt(env: Env, yt_out: i128, v_from_pool: i128, user: Address, max_v_in: i128, vault_rate: i128, amm: Address);
}

#[contractclient(name = "FlashSwapVReceiverClient")]
pub trait FlashSwapVReceiver {
    /// Called by the AMM during `flash_swap_v`. The receiver is lent `pt_borrowed` PT and must,
    /// before returning, deliver `v_owed` vault shares back to `amm`. Anything beyond `v_owed`
    /// is the trade's net output.
    fn on_flash_receive_v(env: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128, vault_rate: i128, amm: Address);
}