import "dotenv/config";
import { PrismaClient, Prisma } from "@prisma/client";
import type { rpc } from "@stellar/stellar-sdk";
import {
    DecodedFactoryEvent,
    decodeFactoryEvent,
    DecodedYmEvent,
    decodeYmEvent,
    DecodedAMMEvent,
    decodeAMMEvent,
} from "./events";
import { getCurrentLedger, getEventsFor } from "./stellar";

const prisma = new PrismaClient();
const FACTORY_ADDRESS = process.env.FACTORY_CONTRACT_ADDRESS!;

function toJsonSafe(value: unknown): Prisma.InputJsonValue {
    return JSON.parse(
        JSON.stringify(value, (_key, v) =>
            typeof v === "bigint" ? v.toString() : v,
        ),
    );
}

async function applyFactoryEvent(
    raw: rpc.Api.EventResponse,
    decoded: DecodedFactoryEvent,
) {
    await prisma.$transaction(async (tx) => {
        const alreadyProcessed = await tx.factoryEvent.findUnique({
            where: { id: raw.id },
        });
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
                        name: decoded.market.name,
                        ym: decoded.market.ym,
                        pt: decoded.market.pt,
                        yt: decoded.market.yt,
                        pool: decoded.market.pool,
                        maturity: decoded.market.maturity,
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

async function applyMarketEvent(
    raw: rpc.Api.EventResponse,
    source: "ym" | "amm",
    decoded: DecodedYmEvent | DecodedAMMEvent,
    marketId: string,
) {
    await prisma.$transaction(async (tx) => {
        const alreadyProcessed = await tx.marketEvent.findUnique({
            where: { id: raw.id },
        });
        if (alreadyProcessed) return;

        await tx.marketEvent.create({
            data: {
                id: raw.id,
                ledger: raw.ledger,
                ledgerClosedAt: new Date(raw.ledgerClosedAt),
                source,
                type: decoded.kind,
                txHash: raw.txHash,
                contractId: raw.contractId!.contractId(),
                market: marketId,
                payload: toJsonSafe(decoded),
            },
        });
    });
}

export async function syncEvents() {
    const state = await prisma.indexerState.upsert({
        where: { id: 1 },
        update: {},
        create: { id: 1 },
    });

    const startLedger = state.lastLedger
        ? state.lastLedger + 1
        : await getCurrentLedger();

    const markets = await prisma.market.findMany({
        select: { id: true, ym: true, pool: true },
    });
    const ymToMarket = new Map(markets.map((market) => [market.ym, market.id]));
    const poolToMarket = new Map(
        markets.map((market) => [market.pool, market.id]),
    );

    const contractIds = [
        FACTORY_ADDRESS,
        ...ymToMarket.keys(),
        ...poolToMarket.keys(),
    ];
    const rawEvents = await getEventsFor(contractIds, startLedger);
    rawEvents.sort((a, b) => a.ledger - b.ledger);

    console.log(
        `[${new Date().toISOString()}] Fetched ${rawEvents.length} event(s) from ledger ${startLedger}`,
    );

    let highestLedger = startLedger;

    for (const raw of rawEvents) {
        highestLedger = Math.max(highestLedger, raw.ledger);
        const contractId = raw.contractId!.contractId();
        if (contractId === FACTORY_ADDRESS) {
            await applyFactoryEvent(raw, decodeFactoryEvent(raw));
        } else if (ymToMarket.has(contractId)) {
            await applyMarketEvent(
                raw,
                "ym",
                decodeYmEvent(raw),
                ymToMarket.get(contractId)!,
            );
        } else if (poolToMarket.has(contractId)) {
            await applyMarketEvent(
                raw,
                "amm",
                decodeAMMEvent(raw),
                poolToMarket.get(contractId)!,
            );
        }
    }

    await prisma.indexerState.update({
        where: { id: 1 },
        data: {
            lastPolled: new Date(),
            lastLedger: highestLedger,
        },
    });
}
