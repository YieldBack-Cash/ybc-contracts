# AMM fuzzing

Runs random operation sequences (deposits, withdrawals, swaps, time jumps,
vault-rate changes) against the pool and checks after every step that reserves
match real token balances, tokens are conserved, and the pool can't be bricked.
Harness lives in `src/fuzz_harness.rs`; the same checks also run via proptest
in normal `cargo test -p amm`.

## Running (Linux/WSL only)

```bash
rustup install nightly
cargo install --locked cargo-fuzz

cd contracts/amm/amm
cargo +nightly fuzz run amm_stateful -- -max_total_time=600
```

Crashes land in `fuzz/artifacts/`; reproduce by passing the artifact path to
the same command.