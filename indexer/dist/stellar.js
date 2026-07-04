"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getCurrentLedger = getCurrentLedger;
exports.getFactoryEvents = getFactoryEvents;
require("dotenv/config");
const stellar_sdk_1 = require("@stellar/stellar-sdk");
const server = new stellar_sdk_1.rpc.Server(process.env.SOROBAN_RPC_URL);
const FACTORY_ADDRESS = process.env.FACTORY_CONTRACT_ADDRESS;
const PAGE_LIMIT = 1000;
async function getCurrentLedger() {
    const latest = await server.getLatestLedger();
    return latest.sequence;
}
async function getFactoryEvents(startLedger) {
    const events = [];
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
