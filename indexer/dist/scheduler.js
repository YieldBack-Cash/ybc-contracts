"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
require("dotenv/config");
const bullmq_1 = require("bullmq");
const indexer_1 = require("./indexer");
const connection = {
    host: "localhost",
    port: 6379,
};
const queue = new bullmq_1.Queue("ybc-indexer", { connection });
new bullmq_1.Worker("ybc-indexer", async () => {
    await (0, indexer_1.syncMarkets)();
}, {
    connection,
});
async function start() {
    await queue.add("sync", {}, {
        repeat: { every: 10000 },
        removeOnComplete: true,
        attempts: 3,
        backoff: { type: "exponential", delay: 2000 },
    });
    console.log("YBC Indexer started — polling every 10s");
}
start();
