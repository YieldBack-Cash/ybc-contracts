import { PrismaClient } from "@prisma/client";

const prisma = new PrismaClient();

async function main() {
    const state = await prisma.indexerState.findUnique({ where: { id: 1 } });
    console.log("IndexerState:", state);

    const markets = await prisma.market.findMany({
        orderBy: { createdAt: "desc" },
        take: 5,
    });
    console.log(`\nMarkets (${markets.length}):`);
    for (const m of markets) {
        console.log(`  ${m.id} | ym=${m.ym} pool=${m.pool} maturity=${m.maturity}`);
    }

    const eventCount = await prisma.marketEvent.count();
    console.log(`\nMarketEvent rows: ${eventCount}`);
    const events = await prisma.marketEvent.findMany({
        orderBy: { ledger: "desc" },
        take: 5,
    });
    for (const e of events) {
        console.log(`  [${e.source}] ${e.type} on ${e.market} (ledger ${e.ledger})`);
    }
}

main().finally(() => process.exit(0));
