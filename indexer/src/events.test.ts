import { Keypair, nativeToScVal } from "@stellar/stellar-sdk";
import type { rpc } from "@stellar/stellar-sdk";
import { decodeFactoryEvent, Market } from "./events";
import { describe, it, expect } from "vitest";

function addr(): string {
    return Keypair.random().publicKey();
}

type ScValTypeSpec = any;

const marketTypeSpec: ScValTypeSpec = {
    ym: ["symbol", "address"],
    pt: ["symbol", "address"],
    yt: ["symbol", "address"],
    pool: ["symbol", "address"],
    maturity: ["symbol", "u64"],
    vault: ["symbol", "address"],
};

function marketScVal(market: Market) {
    return nativeToScVal(market, { type: marketTypeSpec });
}

function fixtureEvent(
    topic: ReturnType<typeof nativeToScVal>[],
    value: ReturnType<typeof nativeToScVal>,
): rpc.Api.EventResponse {
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
    } as rpc.Api.EventResponse;
}

describe("decodeFactoryEvent", () => {
    it("decodes market_created", () => {
        const vault = addr();
        const market: Market = {
            name: "Test Market",
            ym: addr(),
            pt: addr(),
            yt: addr(),
            pool: addr(),
            maturity: 1234567890n,
            vault,
        };

        const event = fixtureEvent(
            [
                nativeToScVal("market_created", {
                    type: "symbol",
                }),
                nativeToScVal(vault, { type: "address" }),
            ],
            marketScVal(market),
        );

        expect(decodeFactoryEvent(event)).toEqual({
            kind: "market_created",
            vault,
            market,
        });
    });

    it("decodes admin_changed", () => {
        const oldAdmin = addr();
        const newAdmin = addr();

        const event = fixtureEvent(
            [nativeToScVal("admin_changed", { type: "symbol" })],
            nativeToScVal(
                { old_admin: oldAdmin, new_admin: newAdmin },
                {
                    type: {
                        old_admin: ["symbol", "address"],
                        new_admin: ["symbol", "address"],
                    },
                },
            ),
        );

        expect(decodeFactoryEvent(event)).toEqual({
            kind: "admin_changed",
            oldAdmin,
            newAdmin,
        });
    });

    it("decodes wasm_hashes_updated", () => {
        const hashSpec: ScValTypeSpec = {
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

        const event = fixtureEvent(
            [nativeToScVal("wasm_hashes_updated", { type: "symbol" })],
            nativeToScVal(
                { old_hashes: oldHashes, new_hashes: newHashes },
                {
                    type: {
                        old_hashes: ["symbol", hashSpec],
                        new_hashes: ["symbol", hashSpec],
                    },
                },
            ),
        );
        const decoded = decodeFactoryEvent(event);
        expect(decoded.kind).toBe("wasm_hashes_updated");
    });

    it("decodes contract_upgraded", () => {
        const newWasmHash = Buffer.alloc(32, 3);

        const event = fixtureEvent(
            [nativeToScVal("contract_upgraded", { type: "symbol" })],
            nativeToScVal(
                { new_wasm_hash: newWasmHash },
                { type: { new_Wasm_hash: ["symbol", "bytes"] } },
            ),
        );

        const decoded = decodeFactoryEvent(event);
        expect(decoded.kind).toBe("contract_upgraded");
    });

    it("throws an unrecognized event name", () => {
        const event = fixtureEvent(
            [nativeToScVal("something_else", { type: "symbol" })],
            nativeToScVal({}),
        );

        expect(() => decodeFactoryEvent(event)).toThrow(
            /Unknown factory event/,
        );
    });
});
