# Event-driven factory indexer

## Problem

The indexer (`indexer/src/`) currently syncs by simulating `get_vaults()` and
`get_markets(vault)` against the factory contract every 10s, re-fetching the
entire state on every tick. The factory contract (as of commit `1c28f1c`) now
emits typed events (`MarketCreated`, `MarketRolledOver`, `AdminChanged`,
`WasmHashesUpdated`, `ContractUpgraded`) that the indexer never reads. We want
the indexer to sync from these events instead of re-fetching full state.

## Constraints

- Soroban RPC has no push/subscribe mechanism. `getEvents` is request/response
  only. "Event-driven" here means cursor-based incremental polling of
  `getEvents`, not a live push feed.
- RPC event retention is finite (provider-dependent, hours to ~7 days).
  Resuming after a longer outage than the retention window is out of scope —
  this is a fresh testnet setup, not a production system with a backfill
  requirement.
- Only the factory contract emits events today. Vault/yield-manager/AMM
  contracts are out of scope for this change.

## Design

### Sync loop

Keep the existing BullMQ repeat job in `scheduler.ts`, but replace the job
body: instead of `syncMarket()` (full state re-fetch), call a new
`syncFactoryEvents()` that:

1. Reads `lastLedger` cursor from `IndexerState`.
2. Calls `server.getEvents({ startLedger: lastLedger ?? currentLedger, filters: [{ type: "contract", contractIds: [FACTORY_ADDRESS] }] })`,
   paginating via the response `cursor` if more events exist than the page limit.
3. Decodes each event (see "Event decoding" below) and applies it inside a DB
   transaction that also inserts the audit row (see "Idempotency").
4. Advances `lastLedger` to the highest ledger processed.

Interval can drop from 10s to ~5s since each tick is now one cheap RPC call
instead of N simulated contract calls.

### Idempotency & crash recovery

Resume at `lastLedger` **inclusive** (not +1). Every decoded event is recorded
in a new `FactoryEvent` table keyed by the RPC's own unique event `id`.
Applying an event = one transaction that inserts the `FactoryEvent` row
(no-op if it already exists) and applies the corresponding mutation. This
makes replaying the same ledger range after a crash safe — no duplicate
writes, no careful off-by-one reasoning required.

### Event decoding

The `#[contractevent]` macro (soroban-sdk 25.3.1) encodes:
- `topic[0]` = snake_case event name as a `Symbol` (e.g. `market_created`).
- Fields marked `#[topic]` follow in the topic array (e.g. `vault: Address`).
- Remaining fields go into `value`, either as a single value
  (`data_format = "single-value"`, used by `MarketCreated`) or as a map
  (the default, used by the other four events).

| Event | Topics | Value | DB action |
|---|---|---|---|
| `market_created` | `[Symbol, Address(vault)]` | `Market` | upsert `Vault`, insert `Market`, insert `FactoryEvent` |
| `market_rolled_over` | `[Symbol, Address(vault)]` | `{old_market, new_market}` | insert new `Market`, insert `FactoryEvent` |
| `admin_changed` | `[Symbol]` | `{old_admin, new_admin}` | insert `FactoryEvent` only |
| `wasm_hashes_updated` | `[Symbol]` | `{old_hashes, new_hashes}` | insert `FactoryEvent` only |
| `contract_upgraded` | `[Symbol]` | `{new_wasm_hash}` | insert `FactoryEvent` only |

### Data model changes (`prisma/schema.prisma`)

- `IndexerState`: add `lastLedger Int?` (nullable until first successful run).
- New `FactoryEvent` model:
  ```
  id             String   @id   // RPC event id, globally unique & ledger-ordered
  ledger         Int
  ledgerClosedAt DateTime
  type           String         // snake_case event name
  txHash         String?
  vault          String?        // set only for market_created / market_rolled_over
  payload        Json
  createdAt      DateTime @default(now())
  ```
- `Market.isActive`: **remove the stored column.** Compute
  `isActive = maturity > now()` at query time in `api.ts` instead. Same
  formula the current code already uses on every poll tick, just evaluated
  lazily instead of cached — this removes the staleness window that exists
  today (up to ~10s where a matured market could still read as active) rather
  than introducing new behavior.

### What gets removed

- `stellar.ts`: `getVaults` / `getMarkets` (the simulated view-call helpers)
  are no longer used by the sync path once this ships, and should be deleted
  rather than left as dead code.
- `indexer.ts`: `syncMarket()` is replaced by `syncFactoryEvents()`.

## Testing plan

1. **Unit tests** for the event decoder/handlers against hand-built fixture
   events (constructed via `stellar-sdk`'s XDR builders) — no network needed,
   catches decoding bugs in isolation.
2. **Manual end-to-end**: redeploy the factory locally (now emitting events),
   call `create_market`, run one sync tick, verify Postgres + `/markets`.
3. **Catch-up test**: stop the indexer, create another market while it's
   down, restart, confirm the cursor resumes and the missed event is picked
   up (not just live-tailing).
4. **Idempotency test**: reprocess the same ledger range twice, confirm no
   duplicate `Market`/`FactoryEvent` rows.

## Out of scope

- Backfilling pre-event on-chain state (this is a fresh testnet deploy).
- Events from contracts other than the factory.
- Any change to how markets are created/rolled over on-chain.
