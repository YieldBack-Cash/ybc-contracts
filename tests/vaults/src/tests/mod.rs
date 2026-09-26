#![cfg(test)]

//! YBC markets created through the real factory on the real vault adapter
//! binaries (`wasms/blend_vault.wasm`, `wasms/xoxno_vault.wasm`, built from
//! the `ybc-vaults` workspace). The Blend adapter runs on a real Blend
//! deployment and the XOXNO adapter on a mock controller, both from
//! `vault_testkit::protocols`, the same fixtures the adapters' own suites run
//! on.
//!
//! Every test body takes a `&VaultStack` and is run once per adapter through
//! `on_every_vault!`, so a property of "a YBC market" is asserted against both
//! vaults rather than against one and assumed for the other.

/// Emits `blend::<name>` and `xoxno::<name>` tests for each listed body.
macro_rules! on_every_vault {
    ($($name:ident),* $(,)?) => {
        mod blend {
            $(
                #[test]
                fn $name() {
                    let env = soroban_sdk::Env::default();
                    super::$name(&super::VaultStack::blend(&env));
                }
            )*
        }
        mod xoxno {
            $(
                #[test]
                fn $name() {
                    let env = soroban_sdk::Env::default();
                    super::$name(&super::VaultStack::xoxno(&env));
                }
            )*
        }
    };
}

mod fixture;

mod asset_paths;
mod split;
mod yield_flow;
