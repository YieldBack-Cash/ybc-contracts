import "dotenv/config";
import express from "express";
import cors from "cors";
import { PrismaClient, Market } from "@prisma/client";

const app = express();
app.use(
    cors({ origin: process.env.FRONTEND_ORIGIN ?? "http://localhost:3000" }),
);
const prisma = new PrismaClient();

// Evaluated per request so `isActive` reflects the current time, not server
// boot time.
function nowSecs(): bigint {
    return BigInt(Math.floor(Date.now() / 1000));
}

function toMarketJson(market: Market, now: bigint) {
    return {
        ...market,
        maturity: market.maturity.toString(),
        isActive: market.maturity > now,
    };
}

app.get("/markets", async (req, res) => {
    const now = nowSecs();
    const markets = await prisma.market.findMany({
        where: { maturity: { gt: now } },
        orderBy: { maturity: "asc" },
    });

    res.json(markets.map((m) => toMarketJson(m, now)));
});

app.get("/markets/:id/events", async (req, res) => {
    const events = await prisma.marketEvent.findMany({
        where: { market: req.params.id },
        orderBy: { ledger: "desc" },
    });
    res.json(events);
});

app.get("/vaults/:address/markets", async (req, res) => {
    const now = nowSecs();
    const markets = await prisma.market.findMany({
        where: { vault: req.params.address },
        orderBy: { maturity: "asc" },
    });

    res.json(markets.map((m) => toMarketJson(m, now)));
});

app.get("/vaults", async (req, res) => {
    const now = nowSecs();
    const vaults = await prisma.vault.findMany({
        include: { markets: true },
    });

    res.json(
        vaults.map((vault) => ({
            ...vault,
            markets: vault.markets.map((m) => toMarketJson(m, now)),
        })),
    );
});

app.get("/status", async (req, res) => {
    const state = await prisma.indexerState.findUnique({ where: { id: 1 } });

    res.json({
        lastPolled: state?.lastPolled ?? null,
        lastLedger: state?.lastLedger ?? null,
    });
});

app.get("/events", async (req, res) => {
    const limit = Math.min(Number(req.query.limit) || 100, 500);
    const events = await prisma.factoryEvent.findMany({
        orderBy: { ledger: "desc" },
        take: limit,
    });
    res.json(events);
});

app.get("/vaults/:address/events", async (req, res) => {
    const events = await prisma.factoryEvent.findMany({
        where: { vault: req.params.address },
        orderBy: { ledger: "desc" },
    });
    res.json(events);
});

app.listen(3001, () => console.log("YBC API running on :3001"));
