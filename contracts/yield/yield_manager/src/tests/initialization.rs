use soroban_sdk::{Address, IntoVal, Symbol};

use super::fixture::YieldManagerTest;

#[test]
fn test_initialization() {
    let test = YieldManagerTest::setup();

    let vault_addr: Address = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_vault"),
        ().into_val(&test.env),
    );
    assert_eq!(vault_addr, test.vault_addr);

    let maturity: u64 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_maturity"),
        ().into_val(&test.env),
    );
    assert_eq!(maturity, test.maturity);
}