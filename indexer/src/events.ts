import { scValToNative } from "@stellar/stellar-sdk";
import type { rpc } from "@stellar/stellar-sdk";

export interface Market {
    name: string;
    ym: string;
    pt: string;
    yt: string;
    pool: string;
    maturity: bigint;
    vault: string;
}

export interface WasmHashes {
    pt: string;
    yt: string;
    ym: string;
    amm: string;
}

export type DecodedFactoryEvent =
    | { kind: "market_created"; vault: string; market: Market }
    | {
          kind: "market_rolled_over";
          vault: string;
          oldMarket: Market;
          newMarket: Market;
      }
    | { kind: "admin_changed"; oldAdmin: string; newAdmin: string }
    | {
          kind: "wasm_hashes_updated";
          old_hashes: WasmHashes;
          new_hashes: WasmHashes;
      }
    | { kind: "contract_upgraded"; new_wasm_hash: string };

export function decodeFactoryEvent(
    raw: rpc.Api.EventResponse,
): DecodedFactoryEvent {
    const topics = raw.topic.map(scValToNative);
    const value = scValToNative(raw.value);
    const eventName = topics[0] as string;

    switch (eventName) {
        case "market_created":
            return {
                kind: "market_created",
                vault: topics[1] as string,
                market: value as Market,
            };
        case "market_rolled_over":
            return {
                kind: "market_rolled_over",
                vault: topics[1] as string,
                oldMarket: value.old_market as Market,
                newMarket: value.new_market as Market,
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
