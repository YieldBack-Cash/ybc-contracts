"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
require("dotenv/config");
const express_1 = __importDefault(require("express"));
const client_1 = require("@prisma/client");
const app = (0, express_1.default)();
const prisma = new client_1.PrismaClient();
const NOW = BigInt(Math.floor(Date.now() / 1000));
function toMarketJson(market) {
    return {
        ...market,
        maturity: market.maturity.toString(),
        isActive: market.maturity > NOW,
    };
}
app.get("/markets", async (req, res) => {
    const markets = await prisma.market.findMany({
        where: { maturity: { gt: NOW } },
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
    res.json(vaults.map((vault) => ({
        ...vault,
        markets: vault.markets.map(toMarketJson),
    })));
});
app.get("/status", async (req, res) => {
    const state = await prisma.indexerState.findUnique({ where: { id: 1 } });
    res.json({
        lastPolled: state?.lastPolled ?? null,
        lastLedger: state?.lastLedger ?? null,
    });
});
app.listen(3001, () => console.log("YBC API running on :3001"));
