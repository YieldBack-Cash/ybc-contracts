#![cfg(test)]

//! YBC markets created through the real factory on the real vault adapter
//! binaries (`wasms/blend_vault.wasm`, `wasms/xoxno_vault.wasm`, built from
//! the `ybc-vaults` workspace). The Blend adapter runs on a real Blend
//! deployment from `wasms/blend/`; the XOXNO adapter on a mock controller that
//! reproduces the two behaviours its correctness depends on.
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

mod blend_protocol;
mod fixture;
mod mock_controller;

mod asset_paths;
mod split;
mod yield_flow;
