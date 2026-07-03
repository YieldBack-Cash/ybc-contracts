# Event-Driven Factory Indexer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Exception for this project:** the user is implementing this plan themselves. Do not dispatch subagents or edit files on their behalf — present each task's code in chat, verify their result, then move to the next task.

**Goal:** Replace the indexer's full-state polling (`get_vaults`/`get_markets` simulated calls every 10s) with cursor-based sync from the factory contract's `#[contractevent]` events.

**Architecture:** The BullMQ scheduler keeps running on an interval, but its job body changes from "re-fetch everything" to "fetch new events since the last processed ledger, decode them, apply them." A `lastLedger` cursor in `IndexerState` and a `FactoryEvent` audit table (keyed by the RPC's own event ID) make replays after a crash/restart idempotent.

**Tech Stack:** TypeScript, `@stellar/stellar-sdk` ^16.0.1, Prisma 5.22 + `@prisma/adapter-pg`, BullMQ, vitest (new — no test framework exists yet).

## Global Constraints

- Soroban RPC's `getEvents` is request/response only — no push/subscribe. This is still polling, just of a cheap incremental diff instead of full state.
- Only the factory contract emits events today; this plan does not touch vault/yield-manager/AMM contracts.
- No backfill logic — treat this as a fresh deploy (per the design spec's "Out of scope" section).
- `Market.isActive` becomes a computed value (`maturity > now()`), not a stored column — see design spec section "Data model changes."
- Full design rationale: `docs/superpowers/specs/2026-07-03-event-driven-indexer-design.md`.

---

### Task 1: Event decoder (`indexer/src/events.ts`)

**Files:**
- Create: `indexer/src/events.ts`
- Create: `indexer/src/events.test.ts`
- Modify: `indexer/package.json` (add vitest)

**Interfaces:**
- Produces: `Market` interface (`ym, pt, yt, pool: string`, `maturity: bigint`, `vault: string`), `WasmHashes` interface (`pt, yt, ym, amm: string` — hex-encoded), `DecodedFactoryEvent` discriminated union (`kind: "market_created" | "market_rolled_over" | "admin_changed" | "wasm_hashes_updated" | "contract_upgraded"`), `decodeFactoryEvent(raw: rpc.Api.EventResponse): DecodedFactoryEvent`. Task 4 consumes all of these.

- [ ] **Step 1: Install vitest**

```bash
cd indexer
npm install --save-dev vitest
```

Then add a `test` script to `indexer/package.json`'s `"scripts"` block (alongside the existing `build`/`start`/`dev`/`api` entries):

```json
"test": "vitest run"
```

- [ ] **Step 2: Write the failing tests**

Create `indexer/src/events.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { nativeToScVal, Keypair } from "@stellar/stellar-sdk";
import type { rpc } from "@stellar/stellar-sdk";
import { decodeFactoryEvent, Market } from "./events";

function addr(): string {
    return Keypair.random().publicKey();
}

const marketTypeSpec = {
    ym: ["symbol", "address"],
    pt: ["symbol", "address"],
    yt: ["symbol", "address"],
    pool: ["symbol", "address"],
    maturity: ["symbol", "u64"],
    vault: ["symbol", "address"],
} as const;

function marketScVal(market: Market) {
    return nativeToScVal(market, { type: marketTypeSpec });
}

function fixtureEvent(
    topic: ReturnType<typeof nativeToScVal>[],
    value: ReturnType<typeof nativeToScVal>,
): rpc.Api.EventResponse {
    return {
        id: "0000000100000000-0000000000",
        type: "contract",
        ledger: 100,
        ledgerClosedAt: new Date().toISOString(),
        transactionIndex: 1,
        operationIndex: 0,
        inSuccessfulContractCall: true,
        txHash: "deadbeef",
        topic,
        value,
    } as rpc.Api.EventResponse;
}

describe("decodeFactoryEvent", () => {
    it("decodes market_created", () => {
        const vault = addr();
        const market: Market = {
            ym: addr(),
            pt: addr(),
            yt: addr(),
            pool: addr(),
            maturity: 1234567890n,
            vault,
        };

        const event = fixtureEvent(
            [nativeToScVal("market_created", { type: "symbol" }), nativeToScVal(vault, { type: "address" })],
            marketScVal(market),
        );

        expect(decodeFactoryEvent(event)).toEqual({ kind: "market_created", vault, market });
    });

    it("decodes market_rolled_over", () => {
        const vault = addr();
        const oldMarket: Market = { ym: addr(), pt: addr(), yt: addr(), pool: addr(), maturity: 100n, vault };
        const newMarket: Market = { ym: addr(), pt: addr(), yt: addr(), pool: addr(), maturity: 200n, vault };

        const event = fixtureEvent(
            [nativeToScVal("market_rolled_over", { type: "symbol" }), nativeToScVal(vault, { type: "address" })],
            nativeToScVal(
                { old_market: oldMarket, new_market: newMarket },
                { type: { old_market: ["symbol", marketTypeSpec], new_market: ["symbol", marketTypeSpec] } },
            ),
        );

        expect(decodeFactoryEvent(event)).toEqual({ kind: "market_rolled_over", vault, oldMarket, newMarket });
    });

    it("decodes admin_changed", () => {
        const oldAdmin = addr();
        const newAdmin = addr();

        const event = fixtureEvent(
            [nativeToScVal("admin_changed", { type: "symbol" })],
            nativeToScVal(
                { old_admin: oldAdmin, new_admin: newAdmin },
                { type: { old_admin: ["symbol", "address"], new_admin: ["symbol", "address"] } },
            ),
        );

        expect(decodeFactoryEvent(event)).toEqual({ kind: "admin_changed", oldAdmin, newAdmin });
    });

    it("decodes wasm_hashes_updated", () => {
        const hashSpec = { pt: ["symbol", "bytes"], yt: ["symbol", "bytes"], ym: ["symbol", "bytes"], amm: ["symbol", "bytes"] } as const;
        const oldHashes = { pt: Buffer.alloc(32, 1), yt: Buffer.alloc(32, 1), ym: Buffer.alloc(32, 1), amm: Buffer.alloc(32, 1) };
        const newHashes = { pt: Buffer.alloc(32, 2), yt: Buffer.alloc(32, 2), ym: Buffer.alloc(32, 2), amm: Buffer.alloc(32, 2) };

        const event = fixtureEvent(
            [nativeToScVal("wasm_hashes_updated", { type: "symbol" })],
            nativeToScVal(
                { old_hashes: oldHashes, new_hashes: newHashes },
                { type: { old_hashes: ["symbol", hashSpec], new_hashes: ["symbol", hashSpec] } },
            ),
        );

        const decoded = decodeFactoryEvent(event);
        expect(decoded.kind).toBe("wasm_hashes_updated");
    });

    it("decodes contract_upgraded", () => {
        const newWasmHash = Buffer.alloc(32, 3);

        const event = fixtureEvent(
            [nativeToScVal("contract_upgraded", { type: "symbol" })],
            nativeToScVal({ new_wasm_hash: newWasmHash }, { type: { new_wasm_hash: ["symbol", "bytes"] } }),
        );

        const decoded = decodeFactoryEvent(event);
        expect(decoded.kind).toBe("contract_upgraded");
    });

    it("throws on an unrecognized event name", () => {
        const event = fixtureEvent([nativeToScVal("something_else", { type: "symbol" })], nativeToScVal({}));
        expect(() => decodeFactoryEvent(event)).toThrow(/Unknown factory event/);
    });
});
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
npx vitest run src/events.test.ts
```

Expected: fails with a module-not-found error for `./events` (the file doesn't exist yet).

- [ ] **Step 4: Write the implementation**

Create `indexer/src/events.ts`:

```typescript
import { scValToNative } from "@stellar/stellar-sdk";
import type { rpc } from "@stellar/stellar-sdk";

export interface Market {
    ym: string;
    pt: string;
    yt: string;
    pool: string;
    maturity: bigint;
    vault: string;
}

export interface WasmHashes {
    pt: string;
    yt: string;
    ym: string;
    amm: string;
}

export type DecodedFactoryEvent =
    | { kind: "market_created"; vault: string; market: Market }
    | { kind: "market_rolled_over"; vault: string; oldMarket: Market; newMarket: Market }
    | { kind: "admin_changed"; oldAdmin: string; newAdmin: string }
    | { kind: "wasm_hashes_updated"; oldHashes: WasmHashes; newHashes: WasmHashes }
    | { kind: "contract_upgraded"; newWasmHash: string };

export function decodeFactoryEvent(raw: rpc.Api.EventResponse): DecodedFactoryEvent {
    const topics = raw.topic.map(scValToNative);
    const value = scValToNative(raw.value);
    const eventName = topics[0] as string;

    switch (eventName) {
        case "market_created":
            return { kind: "market_created", vault: topics[1] as string, market: value as Market };
        case "market_rolled_over":
            return {
                kind: "market_rolled_over",
                vault: topics[1] as string,
                oldMarket: value.old_market as Market,
                newMarket: value.new_market as Market,
            };
        case "admin_changed":
            return { kind: "admin_changed", oldAdmin: value.old_admin, newAdmin: value.new_admin };
        case "wasm_hashes_updated":
            return { kind: "wasm_hashes_updated", oldHashes: value.old_hashes, newHashes: value.new_hashes };
        case "contract_upgraded":
            return { kind: "contract_upgraded", newWasmHash: value.new_wasm_hash };
        default:
            throw new Error(`Unknown factory event: ${eventName}`);
    }
}
```

**Why this shape:** the `#[contractevent]` macro (verified against `soroban-sdk-macros-25.3.1/src/derive_event.rs`) puts the struct name as a snake_case `Symbol` in `topic[0]`, `#[topic]`-marked fields follow in the topic array, and the remaining fields land in `value` — either directly (`MarketCreated`'s `data_format = "single-value"`) or as a map keyed by the Rust field names (the default for the other four events). `scValToNative` collapses both Symbol and String map keys to plain JS strings, so decoding doesn't need to distinguish them.

- [ ] **Step 5: Run tests to verify they pass**

```bash
npx vitest run src/events.test.ts
```

Expected: all 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add indexer/src/events.ts indexer/src/events.test.ts indexer/package.json indexer/package-lock.json
git commit -m "feat: add factory event decoder"
```

---

### Task 2: Schema changes (cursor + audit table, drop stored isActive)

**Files:**
- Modify: `indexer/prisma/schema.prisma`
- Create: new migration (generated by Prisma CLI, do not hand-write)

**Interfaces:**
- Produces: `prisma.indexerState.lastLedger: number | null`, `prisma.factoryEvent` model with fields `id, ledger, ledgerClosedAt, type, txHash, vault, payload, createdAt`. `Market` no longer has `isActive`. Tasks 4 and 6 depend on this.

- [ ] **Step 1: Edit the schema**

In `indexer/prisma/schema.prisma`, change the `IndexerState` model:

```prisma
model IndexerState {
  id         Int      @id @default(1)
  lastPolled DateTime @default(now())
  lastLedger Int?
}
```

Remove the `isActive` line from `Market` (delete `isActive Boolean  @default(true)` — the rest of the model stays the same).

Add a new model at the end of the file:

```prisma
model FactoryEvent {
  id             String   @id
  ledger         Int
  ledgerClosedAt DateTime
  type           String
  txHash         String?
  vault          String?
  payload        Json
  createdAt      DateTime @default(now())
}
```

- [ ] **Step 2: Generate and run the migration**

```bash
cd indexer
npx prisma migrate dev --name event_driven_indexer
```

Expected: Prisma prints the generated SQL (an `ALTER TABLE "IndexerState" ADD COLUMN "lastLedger"...`, an `ALTER TABLE "Market" DROP COLUMN "isActive"`, and a `CREATE TABLE "FactoryEvent"...`), applies it to your local Postgres, and regenerates the Prisma client.

- [ ] **Step 3: Verify the client picked up the new model**

```bash
node -e "const {PrismaClient} = require('@prisma/client'); const p = new PrismaClient(); console.log(typeof p.factoryEvent.create)"
```

Expected output: `function`

- [ ] **Step 4: Commit**

```bash
git add indexer/prisma/schema.prisma indexer/prisma/migrations
git commit -m "feat: add lastLedger cursor and FactoryEvent audit table, drop stored isActive"
```

---

### Task 3: Event fetching (`indexer/src/stellar.ts`)

**Files:**
- Modify: `indexer/src/stellar.ts` (full rewrite — the simulated view-call helpers are no longer used by anything after Task 4)

**Interfaces:**
- Consumes: `process.env.SOROBAN_RPC_URL`, `process.env.FACTORY_CONTRACT_ADDRESS` (already in `.env`).
- Produces: `getCurrentLedger(): Promise<number>`, `getFactoryEvents(startLedger: number): Promise<rpc.Api.EventResponse[]>`. Task 4 consumes both.

- [ ] **Step 1: Replace the file contents**

Replace all of `indexer/src/stellar.ts` with:

```typescript
import "dotenv/config";
import { rpc } from "@stellar/stellar-sdk";

const server = new rpc.Server(process.env.SOROBAN_RPC_URL!);
const FACTORY_ADDRESS = process.env.FACTORY_CONTRACT_ADDRESS!;
const PAGE_LIMIT = 1000;

export async function getCurrentLedger(): Promise<number> {
    const latest = await server.getLatestLedger();
    return latest.sequence;
}

export async function getFactoryEvents(startLedger: number): Promise<rpc.Api.EventResponse[]> {
    const events: rpc.Api.EventResponse[] = [];

    let response = await server.getEvents({
        filters: [{ type: "contract", contractIds: [FACTORY_ADDRESS] }],
        startLedger,
        limit: PAGE_LIMIT,
    });
    events.push(...response.events);

    while (response.events.length === PAGE_LIMIT) {
        response = await server.getEvents({
            filters: [{ type: "contract", contractIds: [FACTORY_ADDRESS] }],
            cursor: response.cursor,
            limit: PAGE_LIMIT,
        });
        events.push(...response.events);
    }

    return events;
}
```

**Why the pagination loop:** `getEvents` caps results per call (we set `PAGE_LIMIT = 1000`). If a response comes back exactly at the limit, there may be more events waiting, so we re-fetch using the response's `cursor` (not `startLedger` — the SDK's `GetEventsRequest` type forbids setting both). A short response (fewer than `PAGE_LIMIT` events) means we've reached the end.

This file has no unit test — it's a thin wrapper around two RPC calls, and the meaningful behavior (does the network actually return the events we expect) is exercised in Task 7's manual end-to-end test, not mocked here.

- [ ] **Step 2: Type-check it compiles**

```bash
cd indexer
npx tsc --noEmit
```

Expected: no errors referencing `stellar.ts`. (There will still be errors in `indexer.ts` referencing the now-deleted `getVaults`/`getMarkets` — that's expected, Task 4 fixes it.)

- [ ] **Step 3: Commit**

```bash
git add indexer/src/stellar.ts
git commit -m "feat: replace simulated view calls with getEvents-based fetching"
```

---

### Task 4: Sync logic (`indexer/src/indexer.ts`)

**Files:**
- Modify: `indexer/src/indexer.ts` (full rewrite)

**Interfaces:**
- Consumes: `getCurrentLedger`, `getFactoryEvents` (Task 3), `decodeFactoryEvent`, `DecodedFactoryEvent` (Task 1), `prisma.indexerState`, `prisma.factoryEvent`, `prisma.market`, `prisma.vault` (Task 2).
- Produces: `syncFactoryEvents(): Promise<void>`. Task 5 consumes this.

- [ ] **Step 1: Replace the file contents**

Replace all of `indexer/src/indexer.ts` with:

```typescript
import "dotenv/config";
import { PrismaClient, Prisma } from "@prisma/client";
import { getCurrentLedger, getFactoryEvents } from "./stellar";
import { decodeFactoryEvent, DecodedFactoryEvent } from "./events";
import type { rpc } from "@stellar/stellar-sdk";

const prisma = new PrismaClient();

function toJsonSafe(value: unknown): Prisma.InputJsonValue {
    return JSON.parse(JSON.stringify(value, (_key, v) => (typeof v === "bigint" ? v.toString() : v)));
}

async function applyEvent(raw: rpc.Api.EventResponse, decoded: DecodedFactoryEvent) {
    await prisma.$transaction(async (tx) => {
        const alreadyProcessed = await tx.factoryEvent.findUnique({ where: { id: raw.id } });
        if (alreadyProcessed) return;

        switch (decoded.kind) {
            case "market_created": {
                await tx.vault.upsert({
                    where: { address: decoded.vault },
                    update: {},
                    create: { address: decoded.vault },
                });
                await tx.market.create({
                    data: {
                        id: `${decoded.vault}:${decoded.market.maturity}`,
                        vault: decoded.vault,
                        ym: decoded.market.ym,
                        pt: decoded.market.pt,
                        yt: decoded.market.yt,
                        pool: decoded.market.pool,
                        maturity: decoded.market.maturity,
                    },
                });
                break;
            }
            case "market_rolled_over": {
                await tx.market.create({
                    data: {
                        id: `${decoded.vault}:${decoded.newMarket.maturity}`,
                        vault: decoded.vault,
                        ym: decoded.newMarket.ym,
                        pt: decoded.newMarket.pt,
                        yt: decoded.newMarket.yt,
                        pool: decoded.newMarket.pool,
                        maturity: decoded.newMarket.maturity,
                    },
                });
                break;
            }
            case "admin_changed":
            case "wasm_hashes_updated":
            case "contract_upgraded":
                break; // audit-only: recorded below, no vault/market row to touch
        }

        await tx.factoryEvent.create({
            data: {
                id: raw.id,
                ledger: raw.ledger,
                ledgerClosedAt: new Date(raw.ledgerClosedAt),
                type: decoded.kind,
                txHash: raw.txHash,
                vault: "vault" in decoded ? decoded.vault : null,
                payload: toJsonSafe(decoded),
            },
        });
    });
}

export async function syncFactoryEvents() {
    const state = await prisma.indexerState.upsert({
        where: { id: 1 },
        update: {},
        create: { id: 1 },
    });

    const startLedger = state.lastLedger ?? (await getCurrentLedger());
    const rawEvents = await getFactoryEvents(startLedger);

    console.log(`[${new Date().toISOString()}] Fetched ${rawEvents.length} event(s) from ledger ${startLedger}`);

    let highestLedger = startLedger;

    for (const raw of rawEvents) {
        const decoded = decodeFactoryEvent(raw);
        await applyEvent(raw, decoded);
        highestLedger = Math.max(highestLedger, raw.ledger);
    }

    await prisma.indexerState.update({
        where: { id: 1 },
        data: { lastPolled: new Date(), lastLedger: highestLedger },
    });
}
```

**Why `lastLedger` only advances after the whole batch:** if the process crashes partway through the `for` loop, `lastLedger` is never updated, so the next run re-fetches the same batch from `startLedger` again. That's fine — `applyEvent`'s `alreadyProcessed` check (keyed on `raw.id`, inside the same transaction as the mutation) makes re-applying already-committed events a no-op. This is the idempotent-consumer pattern from the design spec, section "Idempotency & crash recovery": we deliberately don't try to get the cursor exactly right on every event, we just make replaying safe.

**Why `market_rolled_over` doesn't touch the old market's row:** `isActive` isn't stored anymore (Task 2/6) — a market's active status is `maturity > now()`, computed at read time, so there's nothing to update on the old row when a new one is created.

- [ ] **Step 2: Type-check it compiles**

```bash
cd indexer
npx tsc --noEmit
```

Expected: no errors. (`api.ts` will still error until Task 6 — expected at this point.)

- [ ] **Step 3: Commit**

```bash
git add indexer/src/indexer.ts
git commit -m "feat: sync from factory events instead of full-state polling"
```

---

### Task 5: Scheduler wiring (`indexer/src/scheduler.ts`)

**Files:**
- Modify: `indexer/src/scheduler.ts`

**Interfaces:**
- Consumes: `syncFactoryEvents` (Task 4).

- [ ] **Step 1: Update the job body and interval**

In `indexer/src/scheduler.ts`, change the import and the two spots that reference the old function/interval:

```typescript
import "dotenv/config";
import { Queue, Worker } from "bullmq";
import { syncFactoryEvents } from "./indexer";

const connection = {
    host: "localhost",
    port: 6379,
};

const queue = new Queue("ybc-indexer", { connection });

new Worker(
    "ybc-indexer",
    async () => {
        await syncFactoryEvents();
    },
    {
        connection,
    },
);

async function start() {
    await queue.add(
        "sync",
        {},
        {
            repeat: { every: 5_000 },
            removeOnComplete: true,
            attempts: 3,
            backoff: { type: "exponential", delay: 2000 },
        },
    );

    console.log("YBC Indexer started — syncing factory events every 5s");
}

start();
```

(The only substantive changes from the current file: the import and the call inside the `Worker`, `repeat.every` dropped from `10_000` to `5_000`, and the log message. Everything else — the BullMQ queue/worker/retry setup — is unchanged, since it's not specific to what the job body does.)

- [ ] **Step 2: Commit**

```bash
git add indexer/src/scheduler.ts
git commit -m "chore: point scheduler at syncFactoryEvents, shorten interval to 5s"
```

---

### Task 6: API changes (`indexer/src/api.ts`)

**Files:**
- Modify: `indexer/src/api.ts`

**Interfaces:**
- Consumes: `prisma.market` (no longer has `isActive`; has `maturity: bigint`).

- [ ] **Step 1: Replace the file contents**

Replace all of `indexer/src/api.ts` with:

```typescript
import "dotenv/config";
import express from "express";
import { PrismaClient, Market } from "@prisma/client";

const app = express();
const prisma = new PrismaClient();

function toMarketJson(market: Market) {
    const now = BigInt(Math.floor(Date.now() / 1000));
    return {
        ...market,
        maturity: market.maturity.toString(),
        isActive: market.maturity > now,
    };
}

app.get("/markets", async (req, res) => {
    const now = BigInt(Math.floor(Date.now() / 1000));
    const markets = await prisma.market.findMany({
        where: { maturity: { gt: now } },
        orderBy: { maturity: "asc" },
    });

    res.json(markets.map(toMarketJson));
});

app.get("/vaults/:address/markets", async (req, res) => {
    const markets = await prisma.market.findMany({
        where: { vault: req.params.address },
        orderBy: { maturity: "asc" },
    });
    res.json(markets.map(toMarketJson));
});

app.get("/vaults", async (req, res) => {
    const vaults = await prisma.vault.findMany({
        include: { markets: true },
    });

    res.json(vaults.map((v) => ({ ...v, markets: v.markets.map(toMarketJson) })));
});

app.get("/status", async (req, res) => {
    const state = await prisma.indexerState.findUnique({ where: { id: 1 } });

    res.json({ lastPolled: state?.lastPolled ?? null, lastLedger: state?.lastLedger ?? null });
});

app.listen(3001, () => console.log("YBC API running on :3001"));
```

**Two things changed beyond the `isActive` migration:**
1. `/markets` now filters by `maturity: { gt: now } }` instead of the old stored `isActive: true` column — same semantics, computed instead of cached (see design spec's `isActive` rationale).
2. Every response now runs `maturity` through `.toString()`. This isn't new scope creep — it fixes a pre-existing bug: `Market.maturity` is a Prisma `BigInt`, and `res.json()` calls `JSON.stringify` under the hood, which **throws** on any `BigInt` it encounters. The current polling version has this exact bug already; it just hasn't been hit yet because you haven't successfully returned a market with `isActive: true` through this endpoint during testing. Task 7's end-to-end test would hit this immediately without the fix, so it's included here.

- [ ] **Step 2: Type-check it compiles**

```bash
cd indexer
npx tsc --noEmit
```

Expected: no errors anywhere in `src/`.

- [ ] **Step 3: Commit**

```bash
git add indexer/src/api.ts
git commit -m "fix: compute isActive at read time, fix BigInt JSON serialization"
```

---

### Task 7: End-to-end verification

**Files:** none (no code changes — this is the manual test pass from the design spec's testing plan, points 2-4).

- [ ] **Step 1: Redeploy the factory contract locally**

Use your existing deployment scripts (`scripts/`) to deploy the current `contracts/factory` build — the one with events — to your local node, and update `FACTORY_CONTRACT_ADDRESS` in `indexer/.env` to the new address.

- [ ] **Step 2: Start Postgres, Redis, and reset indexer state**

Make sure your local Postgres and Redis are running (whatever you used to test the polling version). Since the factory address changed, clear out any rows from the old deployment:

```bash
cd indexer
npx prisma studio
```

(Or via `psql`: `DELETE FROM "Market"; DELETE FROM "Vault"; DELETE FROM "IndexerState"; DELETE FROM "FactoryEvent";`) — this isn't required by the code (a fresh `FACTORY_CONTRACT_ADDRESS` means unrelated vault addresses anyway), but keeps your test output easy to read.

- [ ] **Step 3: Start the scheduler and API**

```bash
cd indexer
npm run dev    # scheduler, in one terminal
npm run api    # API server, in another terminal
```

Expected log from the scheduler on the first tick: `Fetched 0 event(s) from ledger <N>` (nothing's happened on-chain yet).

- [ ] **Step 4: Trigger a `market_created` event**

Call `create_market` on the factory (via your deployment scripts or the `soroban`/`stellar` CLI directly) as the admin account.

- [ ] **Step 5: Confirm the indexer picked it up**

Within one scheduler tick (~5s), you should see a log like `Fetched 1 event(s) from ledger <N>`. Then:

```bash
curl http://localhost:3001/markets
```

Expected: a JSON array with one market, `isActive: true`, `maturity` as a string.

```bash
curl http://localhost:3001/status
```

Expected: `lastLedger` is now a number greater than the ledger the market was created in.

- [ ] **Step 6: Catch-up test**

Stop the scheduler (`Ctrl+C` on the `npm run dev` terminal). Trigger a second `create_market` call (different `maturity`) while it's down. Restart `npm run dev`. Confirm the new market shows up in `/markets` without you doing anything else — this proves the cursor resumes from `lastLedger` and doesn't miss events emitted while the indexer was offline.

- [ ] **Step 7: Idempotency test**

With the scheduler stopped, manually re-trigger a sync from an already-processed ledger to confirm replays are safe:

```bash
cd indexer
node -e "
require('dotenv/config');
const { syncFactoryEvents } = require('./dist/indexer');
(async () => {
  await syncFactoryEvents();
  await syncFactoryEvents();
  console.log('ran twice, check DB for duplicates');
})();
"
```

(Run `npm run build` first if `dist/` isn't up to date.) Then check row counts:

```bash
curl http://localhost:3001/markets | node -e "process.stdin.on('data', d => console.log(JSON.parse(d).length))"
```

Expected: the count matches the number of `create_market` calls you made, not double that — confirms the `FactoryEvent`-keyed dedup in `applyEvent` (Task 4) works.

- [ ] **Step 8: Confirm the non-vault events don't error**

Call `set_admin` or `set_wasm_hashes` on the factory (admin-only entrypoints from the "add missing admin entrypoints" commit). Confirm the next sync tick doesn't throw — these events should decode fine and land only in `FactoryEvent`, with no effect on `/markets` or `/vaults`.

---

## Self-review notes

- **Spec coverage:** architecture change → Tasks 3-5; idempotency → Task 4; data model → Task 2; event→handler mapping → Task 4's `applyEvent` switch; `isActive` decision → Tasks 2 and 6; testing plan's 4 points → Task 1 (unit), Task 7 steps 3-5 (E2E), step 6 (catch-up), step 7 (idempotency). All covered.
- **Type consistency checked:** `DecodedFactoryEvent.kind` values match the `type` string stored in `FactoryEvent.type` (Task 4) and the event names asserted in Task 1's tests (`market_created`, `market_rolled_over`, `admin_changed`, `wasm_hashes_updated`, `contract_upgraded`) — all snake_case, matching the macro's `to_snake_case()` behavior confirmed against the soroban-sdk-macros source.
- **No placeholders:** every step above has complete, runnable code — no "similar to Task N" or "add appropriate handling."
