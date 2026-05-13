#![no_std]

use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "AmmClient")]
pub trait AmmInterface {
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128);
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128);
    fn swap_v_for_yt(env: Env, to: Address, v_in: i128, min_yt_out: i128);
    fn swap_yt_for_v(env: Env, to: Address, yt_in: i128, min_v_out: i128);
    fn flash_swap_pt(env: Env, receiver: Address, pt_to_borrow: i128, user: Address, v_in: i128, min_yt_out: i128);
    fn flash_swap_v(env: Env, receiver: Address, pt_to_borrow: i128, user: Address, min_v_out: i128);
    fn deposit(env: Env, to: Address, desired_a: i128, min_a: i128, desired_b: i128, min_b: i128);
    fn withdraw(env: Env, to: Address, share_amount: i128, min_a: i128, min_b: i128) -> (i128, i128);
    fn get_reserves(env: Env) -> (i128, i128);
    fn balance_shares(env: Env, user: Address) -> i128;
}

#[contractclient(name = "FlashReceiverClient")]
pub trait FlashSwapReceiver {
    fn on_flash_receive(env: Env, pt_borrowed: i128, user: Address, v_in: i128, min_yt_out: i128);
}

#[contractclient(name = "FlashVReceiverClient")]
pub trait FlashSwapVReceiver {
    /// Called by the AMM during `flash_swap_v`. The receiver is lent `pt_borrowed` PT and must,
    /// before returning, deliver `v_owed` vault shares back to the AMM (`env.invoker()` / the AMM
    /// address). Anything beyond `v_owed` is the trade's net output.
    fn on_flash_receive_v(env: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128);
}