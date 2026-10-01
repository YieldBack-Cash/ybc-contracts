# YieldBack.Cash (YBC)

A Soroban smart contract protocol for trading interest rate derivatives on Stellar.

## What is YBC?

YBC allows users to split yield-bearing assets (4626-style vault shares) into two separate tokens:

- **Principal Tokens (PT)** - Claim on principal after maturity, earn a fixed interest rate
- **Yield Tokens (YT)** - Earn all variable yield, speculate and bet on interest rates

## How It Works

1. **Deposit** Vault shares (Yield bearing assets) and **receive** PT and YT representing your collateral
2. **Trade** or hold these tokens based on your strategy:
   - Buy/hold PT for predictable, fixed returns
   - Trade YT to speculate on interest rates
3. **Redeem** PT tokens after maturity for the underlying asset after maturity

## Use Cases

### For Fixed Yield Seekers
Principal tokens mimic zero-coupon bonds. Users can redeem a fixed amount at maturity, and users lock in their interest when they purchase the fixed rate principal tokens.

### For Interest Rate Speculators
Yield tokens allow users to bet on interest rates. Users can increase their exposure to interest rate volatility and bet on future interest rates of the underlying protocols (e.g. Blend).

## Development

### Prerequisites

Rust with the Soroban WebAssembly target, and the Stellar CLI:

```
rustup target add wasm32v1-none
cargo install --locked stellar-cli@27.0.0
```

`rust-toolchain.toml` pins the toolchain (1.99.0) and lists the target, so
`rustup` installs both on first use; the first command is the standard one if
you manage targets yourself. The CLI version is the one CI and the release
build use. A different version can produce different bytes, and the hashes an
auditor compares come from this one.

For the vault market tests only, the
[`ybc-vaults`](https://github.com/YieldBack-Cash/ybc-vaults) repository must
be checked out beside this one.

### Build

```
make build
```

That runs `stellar contract build --optimize --meta source_repo=... --meta
home_domain=...`: the standard Soroban build, a wrapper around
`cargo build --target wasm32v1-none --release` that also runs the SDK's spec
tooling, with the flags the release workflow uses. A bare `cargo build` fails
in soroban-sdk 26's build script, so use the wrapper. The flags matter because
the hash is the contract's identity: `--optimize` and the two stamped
metadata entries all change the bytes, so a build without them does not match
the published release. The binaries land in `target/wasm32v1-none/release/`.

The reference build is Linux, which is where the release workflow and the
Stellar Expert verification run. A Linux build of a tagged commit reproduces
the published hashes byte for byte. A Windows build reproduces eight of the
nine; the factory comes out with its functions in a different order, which is
the same program with a different hash. To check a release from Windows,
compare against the GitHub Actions build rather than a local one.

### Test

```
make test
```

That runs `cargo test --workspace` and then the vault market tests in their
own workspace, `cargo test --manifest-path tests/vaults/Cargo.toml`. Build
first on a fresh checkout: the factory and integration tests deploy the
compiled binaries, so they fail until `make build` has run.

`make fixtures` rewrites the parity fixtures under `protocol/fixtures` after a
change to the yield token's accrual or the AMM's curve; the frontend and
indexer pin their TypeScript to them.

### Formatting and lint

```
make lint
```

CI runs the same two checks: `cargo fmt --all -- --check` and clippy with
`-D warnings`, allowing only the three lints the code disagrees with (argument
counts on the router's API, and the `1_0000000` stroop notation). Run
`cargo fmt --all` to fix formatting. `.gitattributes` fixes line endings to LF
on every checkout, whatever `core.autocrlf` says.