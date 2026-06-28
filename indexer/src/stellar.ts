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
const DUMMY_KEY = process.env.DUMMY_PUBLIC_KEY!;

async function callView(functionName: string, args: xdr.ScVal[] = []) {
    const account = new Account(DUMMY_KEY, "0");
    const contract = new Contract(FACTORY_ADDRESS);
    const tx = new TransactionBuilder(account, {
        fee: "100",

        networkPassphrase: Networks.TESTNET,
    })
        .addOperation(contract.call(functionName, ...args))
        .setTimeout(30)
        .build();
    const result = await server.simulateTransaction(tx);

    if (rpc.Api.isSimulationError(result)) {
        throw new Error(`Simulation failed: ${result.error}`);
    }

    return scValToNative(result.result!.retval);
}

export async function getVaults(): Promise<string[]> {
    return await callView("get_vaults");
}

export async function getMarkets(vaultAddress: string) {
    const vaultScVal = nativeToScVal(vaultAddress, { type: "address" });

    return await callView("get_markets", [vaultScVal]);
}
