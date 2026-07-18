use soroban_sdk::{contractevent, Address};

#[contractevent(topics = ["routed_yt_buy"], data_format = "vec")]
pub struct RoutedYtBuy {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub yt_out: i128,
    pub max_v_in: i128,
}

#[contractevent(topics = ["routed_yt_sell"], data_format = "vec")]
pub struct RoutedYtSell {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub yt_in: i128,
    pub min_v_out: i128,
}

#[contractevent(topics = ["exit_expired"], data_format = "vec")]
pub struct ExitedExpired {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub lp_shares: i128,
    pub pt_redeemed: i128,
    pub shares_out: i128,
}