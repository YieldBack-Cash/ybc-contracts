use soroban_sdk::{contractevent, Address};

#[contractevent(topics = ["token_contracts_set"])]
pub struct TokenContractsSet {
    #[topic]
    pub pt: Address,
    #[topic]
    pub yt: Address,
}

#[contractevent(topics = ["pool_set"])]
pub struct PoolSet {
    #[topic]
    pub pool: Address,
}

#[contractevent(topics = ["deposit"], data_format = "vec")]
pub struct Deposit {
    #[topic]
    pub from: Address,
    pub shares_amount: i128,
    pub mint_amount: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["redeem_combined"], data_format = "vec")]
pub struct RedeemCombined {
    #[topic]
    pub from: Address,
    pub amount: i128,
    pub shares_returned: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["redeem_principal"], data_format = "vec")]
pub struct RedeemPrincipal {
    #[topic]
    pub from: Address,
    pub pt_amount: i128,
    pub shares_returned: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["distribute_yield"], data_format = "vec")]
pub struct DistributeYield {
    #[topic]
    pub to: Address,
    pub shares_amount: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["flash_deposit"], data_format = "vec")]
pub struct FlashDeposit {
    #[topic]
    pub user: Address,
    #[topic]
    pub amm: Address,
    pub yt_out: i128,
    pub v_to_mint: i128,
    pub user_cost: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["flash_redeem"], data_format = "vec")]
pub struct FlashRedeem {
    #[topic]
    pub user: Address,
    #[topic]
    pub amm: Address,
    pub pt_borrowed: i128,
    pub v_owed: i128,
    pub v_to_user: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["surplus_collected"], data_format = "vec")]
pub struct SurplusCollected {
    #[topic]
    pub treasury: Address,
    pub amount: i128,
}