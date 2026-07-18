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
    | { kind: "admin_changed"; oldAdmin: string; newAdmin: string }
    | {
          kind: "wasm_hashes_updated";
          old_hashes: WasmHashes;
          new_hashes: WasmHashes;
      }
    | { kind: "contract_upgraded"; new_wasm_hash: string };

export type DecodedYmEvent =
    | {
          kind: "token_contracts_set";
          pt: string;
          yt: string;
      }
    | {
          kind: "deposit";
          from: string;
          shares_amount: bigint;
          mint_amount: bigint;
          exchange_rate: bigint;
      }
    | {
          kind: "redeem_combined";
          from: string;
          amount: bigint;
          shares_returned: bigint;
          exchange_rate: bigint;
      }
    | {
          kind: "redeem_principal";
          from: string;
          pt_amount: bigint;
          shares_returned: bigint;
          exchange_rate: bigint;
      }
    | {
          kind: "distribute_yield";
          to: string;
          shares_amount: bigint;
          exchange_rate: bigint;
      }
    | {
          kind: "flash_deposit";
          user: string;
          amm: string;
          yt_out: bigint;
          v_to_mint: bigint;
          user_cost: bigint;
          exchange_rate: bigint;
      }
    | {
          kind: "flash_redeem";
          user: string;
          amm: string;
          pt_borrowed: bigint;
          v_owed: bigint;
          v_to_user: bigint;
          exchange_rate: bigint;
      };

export type DecodedAMMEvent =
    | {
          kind: "pool_init";
          token_a: string;
          token_b: string;
          expiry_ts: bigint;
          scalar_root: bigint;
          initial_anchor: bigint;
          fee_rate_root: bigint;
          last_implied_rate: bigint;
      }
    | {
          kind: "swap_v_for_pt";
          to: string;
          v_in: bigint;
          pt_out: bigint;
          new_implied_rate: bigint;
          new_reserve_a: bigint;
          new_reserve_b: bigint;
      }
    | {
          kind: "swap_pt_for_v";
          to: string;
          pt_in: bigint;
          v_out: bigint;
          new_implied_rate: bigint;
          new_reserve_a: bigint;
          new_reserve_b: bigint;
      }
    | {
          kind: "flash_swap_pt";
          receiver: string;
          user: string;
          pt_bought: bigint;
          v_paid: bigint;
          new_implied_rate: bigint;
          new_reserve_a: bigint;
          new_reserve_b: bigint;
      }
    | {
          kind: "flash_swap_v";
          receiver: string;
          user: string;
          pt_borrowed: bigint;
          v_owed: bigint;
          new_implied_rate: bigint;
          new_reserve_a: bigint;
          new_reserve_b: bigint;
      }
    | {
          kind: "deposit";
          to: string;
          amount_a: bigint;
          amount_b: bigint;
          shares_minted: bigint;
          new_reserve_a: bigint;
          new_reserve_b: bigint;
      }
    | {
          kind: "withdraw";
          to: string;
          share_amount: bigint;
          amount_a: bigint;
          amount_b: bigint;
          new_reserve_a: bigint;
          new_reserve_b: bigint;
      };

export function decodeYmEvent(raw: rpc.Api.EventResponse): DecodedYmEvent {
    const topics = raw.topic.map(scValToNative);
    const value = scValToNative(raw.value) as bigint[];

    switch (topics[0] as string) {
        case "token_contracts_set": {
            return {
                kind: "token_contracts_set",
                pt: topics[1],
                yt: topics[2],
            };
        }
        case "deposit": {
            const [shares_amount, mint_amount, exchange_rate] = value;
            return {
                kind: "deposit",
                from: topics[1],
                shares_amount,
                mint_amount,
                exchange_rate,
            };
        }
        case "redeem_combined": {
            const [amount, shares_returned, exchange_rate] = value;
            return {
                kind: "redeem_combined",
                from: topics[1],
                amount,
                shares_returned,
                exchange_rate,
            };
        }
        case "redeem_principal": {
            const [pt_amount, shares_returned, exchange_rate] = value;
            return {
                kind: "redeem_principal",
                from: topics[1],
                pt_amount,
                shares_returned,
                exchange_rate,
            };
        }
        case "distribute_yield": {
            const [shares_amount, exchange_rate] = value;
            return {
                kind: "distribute_yield",
                to: topics[1],
                shares_amount,
                exchange_rate,
            };
        }
        case "flash_deposit": {
            const [yt_out, v_to_mint, user_cost, exchange_rate] = value;
            return {
                kind: "flash_deposit",
                user: topics[1],
                amm: topics[2],
                yt_out,
                v_to_mint,
                user_cost,
                exchange_rate,
            };
        }
        case "flash_redeem": {
            const [pt_borrowed, v_owed, v_to_user, exchange_rate] = value;
            return {
                kind: "flash_redeem",
                user: topics[1],
                amm: topics[2],
                pt_borrowed,
                v_owed,
                v_to_user,
                exchange_rate,
            };
        }
        default:
            throw new Error(`Unknown YM Event: ${topics[0]}`);
    }
}

export function decodeAMMEvent(raw: rpc.Api.EventResponse): DecodedAMMEvent {
    const topics = raw.topic.map(scValToNative);
    const name = topics[0] as string;

    if (name === "pool_init") {
        const val = scValToNative(raw.value) as {
            expiry_ts: bigint;
            scalar_root: bigint;
            initial_anchor: bigint;
            fee_rate_root: bigint;
            last_implied_rate: bigint;
        };
        return {
            kind: "pool_init",
            token_a: topics[1],
            token_b: topics[2],
            ...val,
        };
    }

    const value = scValToNative(raw.value) as bigint[];
    switch (name) {
        case "swap_v_for_pt": {
            const [
                v_in,
                pt_out,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            ] = value;
            return {
                kind: "swap_v_for_pt",
                to: topics[1],
                v_in,
                pt_out,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            };
        }
        case "swap_pt_for_v": {
            const [
                pt_in,
                v_out,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            ] = value;
            return {
                kind: "swap_pt_for_v",
                to: topics[1],
                pt_in,
                v_out,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            };
        }
        case "flash_swap_pt": {
            const [
                pt_bought,
                v_paid,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            ] = value;
            return {
                kind: "flash_swap_pt",
                receiver: topics[1],
                user: topics[2],
                pt_bought,
                v_paid,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            };
        }
        case "flash_swap_v": {
            const [
                pt_borrowed,
                v_owed,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            ] = value;
            return {
                kind: "flash_swap_v",
                receiver: topics[1],
                user: topics[2],
                pt_borrowed,
                v_owed,
                new_implied_rate,
                new_reserve_a,
                new_reserve_b,
            };
        }
        case "deposit": {
            const [
                amount_a,
                amount_b,
                shares_minted,
                new_reserve_a,
                new_reserve_b,
            ] = value;
            return {
                kind: "deposit",
                to: topics[1],
                amount_a,
                amount_b,
                shares_minted,
                new_reserve_a,
                new_reserve_b,
            };
        }
        case "withdraw": {
            const [
                share_amount,
                amount_a,
                amount_b,
                new_reserve_a,
                new_reserve_b,
            ] = value;
            return {
                kind: "withdraw",
                to: topics[1],
                share_amount,
                amount_a,
                amount_b,
                new_reserve_a,
                new_reserve_b,
            };
        }
        default:
            throw new Error(`Unknown AMM event: ${name}`);
    }
}

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
