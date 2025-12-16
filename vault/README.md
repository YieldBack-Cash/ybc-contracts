# 4626 Vault Contract

An ERC-4626 style vault implementation for Soroban, enabling efficient yield generation on deposited assets.

## Overview

This vault contract follows the ERC-4626 tokenized vault interface, allowing users to deposit assets and receive fungible vault shares in return. The vault integrates with yield strategies to generate returns on reserved assets.

## YBC Integration

The vault shares are fully compatible with the YBC protocol. When users deposit assets into the vault, they receive vault share tokens that can be used within the YBC ecosystem, giving users access to interest rate derivatives on defi protocols (ex Blend).

## Features

- **ERC-4626 Compliant**: Implements standard 4626 operations
- **Strategies**: Assets are managed by strategies to lend reserves to protocols such as Blend
- **YBC Compatible**: Vault shares integrate with the YBC protocol

