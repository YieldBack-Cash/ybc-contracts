use soroban_sdk::{contractevent, Address, BytesN};

// Ownership events (transfer started/completed, renounced) are emitted by
// the stellar-access Ownable module itself.

#[contractevent]
pub struct Withdrawal {
    #[topic]
    pub token: Address,
    pub to: Address,
    pub amount: i128,
}

#[contractevent]
pub struct ContractUpgraded {
    pub new_wasm_hash: BytesN<32>,
}