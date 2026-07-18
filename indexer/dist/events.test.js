"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const stellar_sdk_1 = require("@stellar/stellar-sdk");
const events_1 = require("./events");
const vitest_1 = require("vitest");
function addr() {
    return stellar_sdk_1.Keypair.random().publicKey();
}
const marketTypeSpec = {
    ym: ["symbol", "address"],
    pt: ["symbol", "address"],
    yt: ["symbol", "address"],
    pool: ["symbol", "address"],
    maturity: ["symbol", "u64"],
    vault: ["symbol", "address"],
};
function marketScVal(market) {
    return (0, stellar_sdk_1.nativeToScVal)(market, { type: marketTypeSpec });
}
function fixtureEvent(topic, value) {
    return {
        id: "0000000100000000-0000000000",
        type: "contract",
        ledger: 100,
        ledgerClosedAt: new Date().toISOString(),
        transactionIndex: 1,
        operationIndex: 0,
        inSuccessfulContractCall: true,
        txHash: "deadbeef",
        topic,
        value,
    };
}
(0, vitest_1.describe)("decodeFactoryEvent", () => {
    (0, vitest_1.it)("decodes market_created", () => {
        const vault = addr();
        const market = {
            ym: addr(),
            pt: addr(),
            yt: addr(),
            pool: addr(),
            maturity: 1234567890n,
            vault,
        };
        const event = fixtureEvent([
            (0, stellar_sdk_1.nativeToScVal)("market_created", {
                type: "symbol",
            }),
            (0, stellar_sdk_1.nativeToScVal)(vault, { type: "address" }),
        ], marketScVal(market));
        (0, vitest_1.expect)((0, events_1.decodeFactoryEvent)(event)).toEqual({
            kind: "market_created",
            vault,
            market,
        });
    });
    (0, vitest_1.it)("decodes market_rolled_over", () => {
        const vault = addr();
        const oldMarket = {
            ym: addr(),
            pt: addr(),
            yt: addr(),
            pool: addr(),
            maturity: 100n,
            vault,
        };
        const newMarket = {
            ym: addr(),
            pt: addr(),
            yt: addr(),
            pool: addr(),
            maturity: 200n,
            vault,
        };
        const event = fixtureEvent([
            (0, stellar_sdk_1.nativeToScVal)("market_rolled_over", { type: "symbol" }),
            (0, stellar_sdk_1.nativeToScVal)(vault, { type: "address" }),
        ], (0, stellar_sdk_1.nativeToScVal)({ old_market: oldMarket, new_market: newMarket }, {
            type: {
                old_market: ["symbol", marketTypeSpec],
                new_market: ["symbol", marketTypeSpec],
            },
        }));
        (0, vitest_1.expect)((0, events_1.decodeFactoryEvent)(event)).toEqual({
            kind: "market_rolled_over",
            vault,
            oldMarket,
            newMarket,
        });
    });
    (0, vitest_1.it)("decodes admin_changed", () => {
        const oldAdmin = addr();
        const newAdmin = addr();
        const event = fixtureEvent([(0, stellar_sdk_1.nativeToScVal)("admin_changed", { type: "symbol" })], (0, stellar_sdk_1.nativeToScVal)({ old_admin: oldAdmin, new_admin: newAdmin }, {
            type: {
                old_admin: ["symbol", "address"],
                new_admin: ["symbol", "address"],
            },
        }));
        (0, vitest_1.expect)((0, events_1.decodeFactoryEvent)(event)).toEqual({
            kind: "admin_changed",
            oldAdmin,
            newAdmin,
        });
    });
    (0, vitest_1.it)("decodes wasm_hashes_updated", () => {
        const hashSpec = {
            pt: ["symbol", "bytes"],
            yt: ["symbol", "bytes"],
            ym: ["symbol", "bytes"],
            amm: ["symbol", "bytes"],
        };
        const oldHashes = {
            pt: Buffer.alloc(32, 1),
            yt: Buffer.alloc(32, 1),
            ym: Buffer.alloc(32, 1),
            amm: Buffer.alloc(32, 1),
        };
        const newHashes = {
            pt: Buffer.alloc(32, 2),
            yt: Buffer.alloc(32, 2),
            ym: Buffer.alloc(32, 2),
            amm: Buffer.alloc(32, 2),
        };
        const event = fixtureEvent([(0, stellar_sdk_1.nativeToScVal)("wasm_hashes_updated", { type: "symbol" })], (0, stellar_sdk_1.nativeToScVal)({ old_hashes: oldHashes, new_hashes: newHashes }, {
            type: {
                old_hashes: ["symbol", hashSpec],
                new_hashes: ["symbol", hashSpec],
            },
        }));
        const decoded = (0, events_1.decodeFactoryEvent)(event);
        (0, vitest_1.expect)(decoded.kind).toBe("wasm_hashes_updated");
    });
    (0, vitest_1.it)("decodes contract_upgraded", () => {
        const newWasmHash = Buffer.alloc(32, 3);
        const event = fixtureEvent([(0, stellar_sdk_1.nativeToScVal)("contract_upgraded", { type: "symbol" })], (0, stellar_sdk_1.nativeToScVal)({ new_wasm_hash: newWasmHash }, { type: { new_Wasm_hash: ["symbol", "bytes"] } }));
        const decoded = (0, events_1.decodeFactoryEvent)(event);
        (0, vitest_1.expect)(decoded.kind).toBe("contract_upgraded");
    });
    (0, vitest_1.it)("throws an unrecognized event name", () => {
        const event = fixtureEvent([(0, stellar_sdk_1.nativeToScVal)("something_else", { type: "symbol" })], (0, stellar_sdk_1.nativeToScVal)({}));
        (0, vitest_1.expect)(() => (0, events_1.decodeFactoryEvent)(event)).toThrow(/Unknown factory event/);
    });
});
