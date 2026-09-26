# protocol/

The protocol's off-chain contract, in one file: `protocol.ts` holds the
fixed-point conventions, the formulas the apps reproduce from the contracts,
and the indexer API's wire types. `fixtures/` holds JSON the contracts' own
tests write, pinning two of those formulas to the Rust.

This folder is the **master copy**. The frontend (`lib/protocol/`) and the
indexer (`src/protocol/`) each vendor a byte-identical copy and import it as
a local module, so neither app needs a dependency, a build step or this repo
at install time. Each app has:

- `npm run check:protocol`: fails if its copy differs from this folder
  (skips when this repo is not checked out beside it);
- `npm run sync:protocol`: overwrites its copy from this folder.

## Changing a formula

1. Change the Rust.
2. `YBC_WRITE_FIXTURES=1 cargo test -p yield_token -p amm parity` rewrites
   `fixtures/`; without the variable those tests fail while a fixture is stale.
3. Change `protocol.ts` to match.
4. `npm run sync:protocol` in the frontend and the indexer. The frontend's
   `tests/protocolParity.test.ts` checks the TypeScript against the fixtures,
   so a mismatch between steps 2 and 3 fails there.

| In `protocol.ts` | Mirrors |
|---|---|
| `SCALAR_7`, `SCALE` | the 7-decimal scale every amount and rate uses |
| `SECONDS_PER_YEAR` | the AMM's `IMPLIED_RATE_TIME` (365 days) |
| `sharesToAssets`, `assetsToShares` | the yield manager's conversions |
| `yearsUntil`, `ptPriceFromRate` | PT price `e^(-r·t)`, the inverse of the AMM's `implied_rate_to_exchange_rate` |
| `pendingYield`, `claimableYield` | `yield_token::math::pending_yield`, exactly |
| `MarketJson`, `VaultJson`, `MarketEventJson`, ... | the indexer API's responses; its serialisers are typed against them and the frontend reads them |
