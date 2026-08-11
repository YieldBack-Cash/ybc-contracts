#![no_std]

//! A real SEP-56 tokenized vault for tests.
//!
//! Unlike `mock_vault` — which fakes an exchange rate through a setter and
//! never holds an underlying asset — this contract delegates to OpenZeppelin's
//! `stellar_tokens::vault::Vault`, an independent ERC-4626/SEP-56
//! implementation. The rate is real: `total_assets` is simply the vault's
//! balance of the underlying, so the only way to make the rate move is to give
//! the vault more assets, exactly as a yielding vault does.
//!
//! That independence is the point. Exercising the protocol's zaps against a
//! SEP-56 shim written alongside them would prove very little — a misreading of
//! the standard would be baked into both sides and the tests would still pass.
//! Running against someone else's implementation of the same standard is what
//! makes the integration meaningful.
//!
//! Not a substitute for testing against the production vault: this one has no
//! fees, no deposit gate, no withdrawal limits, and never loses value. See
//! docs/SECURITY.md for the properties SEP-56 does not guarantee.

mod contract;

pub use contract::{StandardVault, StandardVaultArgs, StandardVaultClient};
