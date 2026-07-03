import "dotenv/config";
import express from "express";
import { PrismaClient } from "@prisma/client";

const app = express();
const prisma = new PrismaClient();

app.get("/markets", async (req, res) => {
    const markets = await prisma.market.findMany({
        where: { isActive: true },
        orderBy: { maturity: "asc" },
    });

    res.json(markets);
});

app.get("/vaults/:address/markets", async (req, res) => {
    const markets = await prisma.market.findMany({
        where: { vault: req.params.address },
        orderBy: { maturity: "asc" },
    });
    res.json(markets);
});

app.get("/vaults", async (req, res) => {
    const vaults = await prisma.vault.findMany({
        include: { markets: true },
    });

    res.json(vaults);
});

app.get("/status", async (req, res) => {
    const state = await prisma.indexerState.findUnique({ where: { id: 1 } });

    res.json({ lastPolled: state?.lastPolled ?? null });
});

app.listen(3001, () => console.log("YBC API running on :3001"));
