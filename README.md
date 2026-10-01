# YieldBack.Cash

Fixed and variable yield on Stellar. YBC splits a yield-bearing vault position
into two tokens that trade separately.

## Overview

A depositor locks vault shares (SEP-56) into a dated market and receives two
tokens:

| Token | What it is | Who wants it |
|---|---|---|
| **PT** — Principal Token | Redeems 1:1 for the underlying at maturity. Bought at a discount, so the discount is a fixed rate. | Anyone who wants a known return |
| **YT** — Yield Token | Receives all the yield the position earns until maturity. | Anyone taking a view on rates |

PT and YT trade against vault shares in a dedicated AMM, so a fixed rate can be
bought or sold at any time before maturity. After maturity, PT redeems for the
underlying and YT stops accruing.

## Contracts

| Contract | Role |
|---|---|
| `factory` | Creates markets: deploys and wires a PT, YT, yield manager and pool for a (vault, maturity) pair. Permissionless. |
| `router` | User entry point. Deposit, split, combine, swap, provide liquidity and exit in one transaction, with every bound set by the caller. |
| `yield_manager` | Holds the vault shares for a market and keeps the accounting between PT and YT. |
| `principal_token`, `yield_token` | SEP-41 tokens on the OpenZeppelin base ledger. |
| `amm` | PT / vault-share pool with a time-decaying curve, so PT converges to face value at maturity. |
| `treasury` | Collects protocol fees. |
| `common` | Shared constants and TTL policy. |

Interfaces live beside each contract (`*_interface`). The vault adapters the
protocol sits on are a separate repository,
[`ybc-vaults`](https://github.com/YieldBack-Cash/ybc-vaults); their compiled
binaries are vendored under `wasms/` for the integration tests (see
`wasms/MANIFEST.md`).

## Getting started

Requirements: Rust (the toolchain version in `rust-toolchain.toml`) and the Stellar CLI.

```sh
cargo install --locked stellar-cli@27.0.0
```

```sh
make build    # compiles every contract
make test     # builds, then runs every test suite
make lint     # formatting and clippy, as CI runs them
```

The vault market tests under `tests/vaults` need `ybc-vaults` checked out
beside this repository. `make build` runs the release workflow's exact
command, so building a tagged commit reproduces the hashes on the Releases
page.

`make fixtures` regenerates the parity fixtures in `protocol/fixtures` after a
change to the yield token's accrual or the AMM's curve. The frontend and
indexer pin their TypeScript ports to them.

## Deployments

| Network | Addresses |
|---|---|
| Testnet | Redeploy from `v0.1.2` pending; `deployments/deployments.testnet.json` will hold the addresses. |
| Mainnet | Not deployed. |

## Security

The security model and trust assumptions are in
[`docs/SECURITY.md`](docs/SECURITY.md).

Report vulnerabilities: email
[ben@yieldback.cash](mailto:ben@yieldback.cash) or message
[Discord](https://discord.gg/esukQxvMF).

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — contracts, flows and invariants
- [`docs/SECURITY.md`](docs/SECURITY.md) — security model, trust assumptions and guarantees
- [`protocol/`](protocol/) — the off-chain contract: fixed-point conventions, the formulas the apps reproduce, and the indexer's wire types, vendored by the frontend and indexer

## Licence

GPL-3.0. See [`LICENSE.md`](LICENSE.md).
