use crate::contract::{Market, WasmHashes};
use soroban_sdk::{contractevent, Address, BytesN};

#[contractevent(data_format = "single-value")]
pub struct MarketCreated {
    #[topic]
    pub vault: Address,
    pub market: Market,
}

#[contractevent]
pub struct AdminChanged {
    pub old_admin: Address,
    pub new_admin: Address,
}

#[contractevent]
pub struct WasmHashesUpdated {
    pub old_hashes: WasmHashes,
    pub new_hashes: WasmHashes,
}

#[contractevent]
pub struct ContractUpgraded {
    pub new_wasm_hash: BytesN<32>,
}
