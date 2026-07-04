/*
  Warnings:

  - You are about to drop the column `isActive` on the `Market` table. All the data in the column will be lost.

*/
-- AlterTable
ALTER TABLE "IndexerState" ADD COLUMN     "lastLedger" INTEGER;

-- AlterTable
ALTER TABLE "Market" DROP COLUMN "isActive";

-- CreateTable
CREATE TABLE "FactoryEvent" (
    "id" TEXT NOT NULL,
    "ledger" INTEGER NOT NULL,
    "ledgerClosedAt" TIMESTAMP(3) NOT NULL,
    "type" TEXT NOT NULL,
    "txHash" TEXT,
    "vault" TEXT,
    "payload" JSONB NOT NULL,
    "createdAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT "FactoryEvent_pkey" PRIMARY KEY ("id")
);
