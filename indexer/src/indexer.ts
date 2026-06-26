import { PrismaClient } from "@prisma/client";
import { getVaults, getMarkets } from "./stellar";

const prisma = new PrismaClient();

export async function syncMarket() {
    console.log(`[${new Date().toISOString()}] Syncing markets...`);

    const vaults: string[] = await getVaults();

    for (const vaultAddress of vaults) {
        await prisma.vault.upsert({
            where: { address: vaultAddress },

            update: {},
            create: { address: vaultAddress },
        });

        const markets = await getMarkets(vaultAddress);

        for (const market of markets) {
            const id = `${vaultAddress}:${market.maturity}`;

            await prisma.market.upsert({
                where: { id },

                update: {
                    isActive: Date.now() / 1000 < Number(market.maturity),
                },

                create: {
                    id,
                    vault: vaultAddress,

                    ym: market.ym,

                    pt: market.pt,

                    yt: market.yt,

                    pool: market.pool,
                    maturity: BigInt(market.maturity),

                    isActive: Date.now() / 1000 < Number(market.maturity),
                },
            });
        }

        console.log(`  Vault ${vaultAddress}: ${markets.length} markets`);
    }

    await prisma.indexerState.upsert({
        where: { id: 1 },

        update: { lastPolled: new Date() },

        create: { id: 1 },
    });

    console.log(`Done. ${vaults.length} vaults synced.`);
}
