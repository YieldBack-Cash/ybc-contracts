// The YBC protocol's fixed-point conventions, the formulas the off-chain code
// reproduces from the contracts, and the indexer API's wire types.
//
// MASTER COPY: ybc-contracts/protocol/protocol.ts. The frontend
// (lib/protocol/protocol.ts) and the indexer (src/protocol/protocol.ts) hold
// byte-identical copies, kept so by each app's `check:protocol` script and
// refreshed with `sync:protocol`. Edit this file, then sync; never edit a copy.
//
// Two formulas mirror Rust and are pinned to it by fixtures the contracts'
// own tests write into ./fixtures: `pendingYield` reproduces
// `yield_token::math::pending_yield` exactly, and `ptPriceFromRate` is the
// floating-point inverse of the AMM's `implied_rate_to_exchange_rate`.

// ── fixed-point conventions ─────────────────────────────────────────────────

/** Every on-chain amount and rate is a 7-decimal fixed-point integer. */
export const SCALAR_7 = 10_000_000n;
/** The same scale as a number, for display math. */
export const SCALE = 1e7;
/** The AMM's year: 365 days, no leap adjustment (`IMPLIED_RATE_TIME`). */
export const SECONDS_PER_YEAR = 31_536_000;

// ── formulas ────────────────────────────────────────────────────────────────

/** Asset value of `shares` at `rate` (assets per SCALAR_7 shares), floored. */
export function sharesToAssets(shares: bigint, rate: bigint): bigint {
    return (shares * rate) / SCALAR_7;
}

/** Vault shares for `assets` at `rate`, floored, as the yield manager computes them. */
export function assetsToShares(assets: bigint, rate: bigint): bigint {
    return (assets * SCALAR_7) / rate;
}

/** Years from `nowMs` to a maturity in Unix seconds; negative once past it. */
export function yearsUntil(maturitySeconds: number, nowMs: number): number {
    return (maturitySeconds - nowMs / 1000) / SECONDS_PER_YEAR;
}

/**
 * PT's price in the underlying from the pool's implied rate: `e^(-r * t)`.
 *
 * `rawRate` is the pool's 1e7-scaled ln-space rate; the pool's own exchange
 * rate is `e^(r * t)` (fixed-point series, accurate to about 1e-5), and PT
 * redeems one unit of the asset at maturity, so it is worth the inverse now.
 * At or past maturity PT is at par.
 */
export function ptPriceFromRate(rawRate: number, years: number): number {
    return Math.exp(-(rawRate / SCALE) * Math.max(years, 0));
}

/**
 * Vault shares a holder of `balance` YT (asset-denominated) has accrued since
 * they last settled at `userIndex`, now that the rate is `exchangeRate`.
 * Exactly `yield_token::math::pending_yield`: zero when nothing has accrued
 * or the inputs are degenerate, floored otherwise.
 */
export function pendingYield(balance: bigint, userIndex: bigint, exchangeRate: bigint): bigint {
    if (balance <= 0n || userIndex <= 0n || exchangeRate <= userIndex) return 0n;
    return (balance * (exchangeRate - userIndex) * SCALAR_7) / (userIndex * exchangeRate);
}

/** What `claim_yield` would pay right now: already accrued plus pending, in vault shares. */
export function claimableYield(
    balance: bigint,
    accruedYield: bigint,
    userIndex: bigint,
    exchangeRate: bigint,
): bigint {
    return accruedYield + pendingYield(balance, userIndex, exchangeRate);
}

// ── the indexer API's wire types ────────────────────────────────────────────
//
// The indexer's serialisers are typed against these (a column it forgets to
// map, or maps under another name, is a type error there), and the frontend
// reads its responses as these (a field it expects that the indexer stopped
// sending is a type error here). Dates travel as ISO-8601 strings and every
// on-chain integer as a decimal string, because JSON has no BigInt.

/** One PT/YT market, from `/markets`, `/vaults/:address/markets` and inside a vault. */
export interface MarketJson {
    /** `<vault>:<maturity>` */
    id: string;
    vault: string;
    /** The on-chain market name (`bvXLM-23DEC2026`). */
    name: string;
    ym: string;
    pt: string;
    yt: string;
    pool: string;
    /** Unix seconds. */
    maturity: string;
    creator: string | null;
    verified: boolean;
    listed: boolean;
    curatedAt: string | null;
    /** Creator-supplied pool parameters, 1e7-scaled APYs; null before `pool_init` is indexed. */
    currentApy: string | null;
    apyMin: string | null;
    apyMax: string | null;
    feeApy: string | null;
    /** Treasury's share of each trade's fee, 1e7-scaled fraction of the fee. */
    reserveFeeRate: string | null;
    /** 7-day trailing realised LP fee APY, 1e7-scaled, simple; null until the pool has fee-carrying trades. */
    lpFeeApy: string | null;
    lpFeeApyUpdatedAt: string | null;
    createdAt: string;
    updatedAt: string;
    /** Whether maturity is still ahead, evaluated when the response was built. */
    isActive: boolean;
}

/** A vault with its curated metadata, from `/vaults` and `/vaults/:address`. */
export interface VaultJson {
    address: string;
    createdAt: string;
    updatedAt: string;

    // Indexed from the vault contract, best-effort; any may be null.
    underlyingSymbol: string | null;
    underlyingAsset: string | null;
    /** The protocol contract behind the vault (`get_protocol`). */
    pool: string | null;

    // Curated; any may be null until a curator fills it in.
    displayName: string | null;
    description: string | null;
    riskText: string | null;
    protocolId: string | null;
    curatedAt: string | null;
    // The protocol row, flattened.
    protocolName: string | null;
    protocolLogoUrl: string | null;
    protocolWebsite: string | null;
    protocolDocsUrl: string | null;
    protocolAuditUrl: string | null;

    /** Present on the vault endpoints; absent where a vault is nested elsewhere. */
    markets?: MarketJson[];
}

export type MarketEventSource = "ym" | "amm" | "router";

/** One event from a market's contracts, from `/markets/:id/events` and `/accounts/:address/events`. */
export interface MarketEventJson {
    id: string;
    ledger: number;
    ledgerClosedAt: string;
    source: MarketEventSource;
    /** The event's topic (`swap_v_for_pt`, `deposit_asset`, `zap_in`, ...), or `undecoded`. */
    type: string;
    txHash: string | null;
    contractId: string;
    /** `<vault>:<maturity>` */
    market: string;
    /** The decoded event, integers as decimal strings; shape per `type`. */
    payload: unknown;
    createdAt: string;
}

/** One factory event, from `/events` and `/vaults/:address/events`. */
export interface FactoryEventJson {
    id: string;
    ledger: number;
    ledgerClosedAt: string;
    type: string;
    txHash: string | null;
    vault: string | null;
    payload: unknown;
    createdAt: string;
}

/** From `/vaults/:address/rate-history`: assets per vault share, hourly. */
export interface VaultRateSnapshotJson {
    rate: number;
    timestamp: string;
}

/** From `/accounts/:address/balances`: PT and YT held per listed market. */
export interface AccountBalanceJson {
    marketId: string;
    ptBalance: string;
    ytBalance: string;
}

/** From `/status`. */
export interface IndexerStatusJson {
    lastPolled: string | null;
    lastLedger: number | null;
}
