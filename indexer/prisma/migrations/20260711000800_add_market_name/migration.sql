/*
  Warnings:

  - Added the required column `name` to the `Market` table without a default value. This is not possible if the table is not empty.

*/
-- AlterTable
ALTER TABLE "Market" ADD COLUMN     "name" TEXT NOT NULL;
