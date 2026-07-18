-- CreateTable
CREATE TABLE "IndexerState" (
    "id" INTEGER NOT NULL DEFAULT 1,
    "lastPolled" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT "IndexerState_pkey" PRIMARY KEY ("id")
);

-- CreateTable
CREATE TABLE "Vault" (
    "address" TEXT NOT NULL,
    "createdAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT "Vault_pkey" PRIMARY KEY ("address")
);

-- CreateTable
CREATE TABLE "Market" (
    "id" TEXT NOT NULL,
    "vault" TEXT NOT NULL,
    "ym" TEXT NOT NULL,
    "pt" TEXT NOT NULL,
    "yt" TEXT NOT NULL,
    "pool" TEXT NOT NULL,
    "maturity" BIGINT NOT NULL,
    "isActive" BOOLEAN NOT NULL DEFAULT true,
    "createdAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updatedAt" TIMESTAMP(3) NOT NULL,

    CONSTRAINT "Market_pkey" PRIMARY KEY ("id")
);

-- AddForeignKey
ALTER TABLE "Market" ADD CONSTRAINT "Market_vault_fkey" FOREIGN KEY ("vault") REFERENCES "Vault"("address") ON DELETE RESTRICT ON UPDATE CASCADE;
