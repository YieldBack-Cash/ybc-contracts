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

const server = new rpc.Server(process.env.SOROBAN_RPC_URL!);
const FACTORY_ADDRESS = process.env.FACTORY_CONTRACT_ADDRESS!;
const PAGE_LIMIT = 1000;

export async function getCurrentLedger(): Promise<number> {
    const latest = await server.getLatestLedger();
    return latest.sequence;
}

export async function getFactoryEvents(
    startLedger: number,
): Promise<rpc.Api.EventResponse[]> {
    const events: rpc.Api.EventResponse[] = [];

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
