use soroban_sdk::{contractevent, Address};

#[contractevent(topics = ["routed_yt_buy"], data_format = "vec")]
pub struct RoutedYtBuy {
    #[topic]
    pub to: Address,
    pub v_in: i128,
    pub min_yt_out: i128,
    pub pt_to_borrow: i128,
    pub exchange_rate: i128,
}

#[contractevent(topics = ["routed_yt_sell"], data_format = "vec")]
pub struct RoutedYtSell {
    #[topic]
    pub to: Address,
    pub yt_in: i128,
    pub min_v_out: i128,
}