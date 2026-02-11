# Vault Interfaces

This directory contains interface crates for external vault contracts that YBC integrates with.

- **vault_interface** - Interface for ERC-4626 style vaults (e.g. Blend)
- **defindex_interface** - Interface for DeFindex vaults

These are used by the Yield Manager to query vault exchange rates and interact with vault contracts. No vault implementations live here.