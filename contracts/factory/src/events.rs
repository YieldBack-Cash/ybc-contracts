use crate::contract::{FeeConfig, Market, WasmHashes};
use soroban_sdk::{contractevent, Address, BytesN};

#[contractevent(data_format = "single-value")]
pub struct MarketCreated {
    #[topic]
    pub creator: Address,
    #[topic]
    pub vault: Address,
    pub market: Market,
}

// Ownership events (transfer started/completed, renounced) are emitted by
// the stellar-access Ownable module itself.

#[contractevent]
pub struct WasmHashesUpdated {
    pub old_hashes: WasmHashes,
    pub new_hashes: WasmHashes,
}

#[contractevent]
pub struct FeeConfigUpdated {
    pub old_config: FeeConfig,
    pub new_config: FeeConfig,
}

#[contractevent]
pub struct ContractUpgraded {
    pub new_wasm_hash: BytesN<32>,
}
