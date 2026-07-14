-- CreateTable
CREATE TABLE "MarketEvent" (
    "id" TEXT NOT NULL,
    "ledger" INTEGER NOT NULL,
    "ledgerClosedAt" TIMESTAMP(3) NOT NULL,
    "source" TEXT NOT NULL,
    "type" TEXT NOT NULL,
    "txHash" TEXT,
    "contractId" TEXT NOT NULL,
    "market" TEXT NOT NULL,
    "payload" JSONB NOT NULL,
    "createdAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT "MarketEvent_pkey" PRIMARY KEY ("id")
);
