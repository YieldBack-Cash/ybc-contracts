import "dotenv/config";
import {
    rpc,
    TransactionBuilder,
    Networks,
    Account,
    Contract,
    xdr,
    scValToNative,
    nativeToScVal,
} from "@stellar/stellar-sdk";
import { contract } from "@stellar/stellar-sdk/axios";

const server = new rpc.Server(process.env.SOROBAN_RPC_URL!);
const FACTORY_ADDRESS = process.env.FACTORY_CONTRACT_ADDRESS!;
const PAGE_LIMIT = 1000;

export async function getCurrentLedger(): Promise<number> {
    const latest = await server.getLatestLedger();
    return latest.sequence;
}

export async function getEventsFor(
    contractIds: string[],
    startLedger: number,
): Promise<rpc.Api.EventResponse[]> {
    const events: rpc.Api.EventResponse[] = [];
    if (contractIds.length === 0) return events;

    const chunks: string[][] = [];
    for (let i = 0; i < contractIds.length; i += 5) {
        chunks.push(contractIds.slice(i, i + 5));
    }

    for (const chunk of chunks) {
        let response = await server.getEvents({
            filters: [{ type: "contract", contractIds: chunk }],
            startLedger,
            limit: PAGE_LIMIT,
        });
        events.push(...response.events);
        while (response.events.length === PAGE_LIMIT) {
            response = await server.getEvents({
                filters: [{ type: "contract", contractIds: chunk }],
                cursor: response.cursor,
                limit: PAGE_LIMIT,
            });
            events.push(...response.events);
        }
    }
    return events;
}
