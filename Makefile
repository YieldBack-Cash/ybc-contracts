default: build

# `stellar contract build`, not `cargo build`: soroban-sdk 26's spec shaking
# needs the CLI wrapper, and a bare cargo build fails in the SDK's build
# script. The flags are the ones the release workflow uses (`--optimize`, and
# the source repository stamped into the binary), so a local build hashes the
# same as the published release. The test fixtures import the binaries this
# produces, so build before test on a fresh checkout.
build:
	stellar contract build --optimize --meta source_repo=github:YieldBack-Cash/ybc-contracts
	@ls -l target/wasm32v1-none/release/*.wasm

# What CI runs (.github/workflows/ci.yml). tests/vaults is its own workspace
# (it needs ybc-vaults checked out beside this repo), so it is run separately.
test:
	cargo test --workspace
	cargo test --manifest-path tests/vaults/Cargo.toml

# Rewrite the parity fixtures the frontend and indexer pin their TypeScript
# to, after changing the yield token's accrual or the AMM's curve.
fixtures:
	YBC_WRITE_FIXTURES=1 cargo test -p yield_token -p amm parity

fmt:
	cargo fmt --all

clean:
	cargo clean

.PHONY: default build test fixtures fmt lint clean

# What CI gates: formatting, and the linter with the three lints the code
# disagrees with allowed (argument counts on the API, the 1_0000000 stroop
# notation).
lint:
	cargo fmt --all -- --check
	cargo fmt --manifest-path tests/vaults/Cargo.toml -- --check
	cargo clippy --workspace --all-targets -- -D warnings -A clippy::too-many-arguments -A clippy::inconsistent-digit-grouping -A clippy::zero-prefixed-literal
