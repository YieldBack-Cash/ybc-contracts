"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getVaults = getVaults;
exports.getMarkets = getMarkets;
require("dotenv/config");
const stellar_sdk_1 = require("@stellar/stellar-sdk");
const server = new stellar_sdk_1.rpc.Server(process.env.SOROBAN_RPC_URL);
const FACTORY_ADDRESS = process.env.FACTORY_CONTRACT_ADDRESS;
const DUMMY_KEY = process.env.DUMMY_PUBLIC_KEY;
async function callView(functionName, args = []) {
    const account = new stellar_sdk_1.Account(DUMMY_KEY, "0");
    const contract = new stellar_sdk_1.Contract(FACTORY_ADDRESS);
    const tx = new stellar_sdk_1.TransactionBuilder(account, {
        fee: "100",
        networkPassphrase: stellar_sdk_1.Networks.TESTNET,
    })
        .addOperation(contract.call(functionName, ...args))
        .setTimeout(30)
        .build();
    const result = await server.simulateTransaction(tx);
    if (stellar_sdk_1.rpc.Api.isSimulationError(result)) {
        throw new Error(`Simulation failed: ${result.error}`);
    }
    return (0, stellar_sdk_1.scValToNative)(result.result.retval);
}
async function getVaults() {
    return await callView("get_vaults");
}
async function getMarkets(vaultAddress) {
    const vaultScVal = (0, stellar_sdk_1.nativeToScVal)(vaultAddress, { type: "address" });
    return await callView("get_markets", [vaultScVal]);
}
