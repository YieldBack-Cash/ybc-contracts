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
