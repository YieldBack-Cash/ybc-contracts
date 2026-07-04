"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.syncFactoryEvents = syncFactoryEvents;
require("dotenv/config");
const client_1 = require("@prisma/client");
const events_1 = require("./events");
const stellar_1 = require("./stellar");
const prisma = new client_1.PrismaClient();
function toJsonSafe(value) {
    return JSON.parse(JSON.stringify(value, (_key, v) => typeof v === "bigint" ? v.toString() : v));
}
async function applyEvent(raw, decoded) {
    await prisma.$transaction(async (tx) => {
        const alreadyProcessed = await tx.factoryEvent.findUnique({
            where: { id: raw.id },
        });
        if (alreadyProcessed)
            return;
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
                break;
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
async function syncFactoryEvents() {
    const state = await prisma.indexerState.upsert({
        where: { id: 1 },
        update: {},
        create: { id: 1 },
    });
    const startLedger = state.lastLedger ?? (await (0, stellar_1.getCurrentLedger)());
    const rawEvents = await (0, stellar_1.getFactoryEvents)(startLedger);
    console.log(`[${new Date().toISOString()}] Fetched ${rawEvents.length} event(s) from ledger ${startLedger}`);
    let highestLedger = startLedger;
    for (const raw of rawEvents) {
        const decoded = (0, events_1.decodeFactoryEvent)(raw);
        await applyEvent(raw, decoded);
        highestLedger = Math.max(highestLedger, raw.ledger);
    }
    await prisma.indexerState.update({
        where: { id: 1 },
        data: {
            lastPolled: new Date(),
            lastLedger: highestLedger,
        },
    });
}
