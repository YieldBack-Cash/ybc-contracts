"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.decodeFactoryEvent = decodeFactoryEvent;
const stellar_sdk_1 = require("@stellar/stellar-sdk");
function decodeFactoryEvent(raw) {
    const topics = raw.topic.map(stellar_sdk_1.scValToNative);
    const value = (0, stellar_sdk_1.scValToNative)(raw.value);
    const eventName = topics[0];
    switch (eventName) {
        case "market_created":
            return {
                kind: "market_created",
                vault: topics[1],
                market: value,
            };
        case "market_rolled_over":
            return {
                kind: "market_rolled_over",
                vault: topics[1],
                oldMarket: value.old_market,
                newMarket: value.new_market,
            };
        case "admin_changed":
            return {
                kind: "admin_changed",
                oldAdmin: value.old_admin,
                newAdmin: value.new_admin,
            };
        case "wasm_hashes_updated":
            return {
                kind: "wasm_hashes_updated",
                old_hashes: value.old_hashes,
                new_hashes: value.new_hashes,
            };
        case "contract_upgraded":
            return {
                kind: "contract_upgraded",
                new_wasm_hash: value.new_wasm_hash,
            };
        default:
            throw new Error(`Unknown factory event: ${eventName}`);
    }
}
