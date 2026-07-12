use soroban_sdk::{contractevent, Address};

#[contractevent(topics = ["pool_init"])]
pub struct PoolInit {
    #[topic]
    pub token_a: Address,
    #[topic]
    pub token_b: Address,
    pub expiry_ts: u64,
    pub scalar_root: i128,
    pub initial_anchor: i128,
    pub fee_rate_root: i128,
    pub last_implied_rate: i128,
}

#[contractevent(topics = ["swap_v_for_pt"], data_format = "vec")]
pub struct SwapVForPt {
    #[topic]
    pub to: Address,
    pub v_in: i128,
    pub pt_out: i128,
    pub new_implied_rate: i128,
    pub new_reserve_a: i128,
    pub new_reserve_b: i128,
}

#[contractevent(topics = ["swap_pt_for_v"], data_format = "vec")]
pub struct SwapPtForV {
    #[topic]
    pub to: Address,
    pub pt_in: i128,
    pub v_out: i128,
    pub new_implied_rate: i128,
    pub new_reserve_a: i128,
    pub new_reserve_b: i128,
}

#[contractevent(topics = ["flash_swap_pt"], data_format = "vec")]
pub struct FlashSwapPt {
    #[topic]
    pub receiver: Address,
    #[topic]
    pub user: Address,
    pub pt_borrowed: i128,
    pub v_in: i128,
    pub new_implied_rate: i128,
    pub new_reserve_a: i128,
    pub new_reserve_b: i128,
}

#[contractevent(topics = ["flash_swap_v"], data_format = "vec")]
pub struct FlashSwapV {
    #[topic]
    pub receiver: Address,
    #[topic]
    pub user: Address,
    pub pt_borrowed: i128,
    pub v_owed: i128,
    pub new_implied_rate: i128,
    pub new_reserve_a: i128,
    pub new_reserve_b: i128,
}

#[contractevent(topics = ["deposit"], data_format = "vec")]
pub struct Deposit {
    #[topic]
    pub to: Address,
    pub amount_a: i128,
    pub amount_b: i128,
    pub shares_minted: i128,
    pub new_reserve_a: i128,
    pub new_reserve_b: i128,
}

#[contractevent(topics = ["withdraw"], data_format = "vec")]
pub struct Withdraw {
    #[topic]
    pub to: Address,
    pub share_amount: i128,
    pub amount_a: i128,
    pub amount_b: i128,
    pub new_reserve_a: i128,
    pub new_reserve_b: i128,
}