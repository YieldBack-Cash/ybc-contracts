use soroban_sdk::contracterror;

/// Every way a router call can fail on its own checks.
///
/// Typed so a client can tell a slippage bound from an unfunded swap leg:
/// returned through `Result`, these surface as `Error(Contract, #n)` and appear
/// in the contract spec (the release profile strips panic messages, so a bare
/// `assert!` would reach a client as an opaque trap). Failures inside the AMM,
/// vault or yield manager still carry those contracts' own errors.
///
/// Codes are part of the client-facing API: append new variants, never
/// renumber existing ones.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RouterError {
    /// The factory has no market for this (vault, maturity).
    MarketNotFound = 1,
    /// An amount argument is zero or negative where that is not allowed.
    /// Minimum-out bounds must be positive: a zero minimum is no protection,
    /// and the pool rejects it anyway.
    InvalidAmount = 2,
    /// An expired-market exit was called before maturity.
    MarketNotExpired = 3,
    /// The vault deposit minted zero shares.
    VaultMintedNoShares = 4,
    /// The vault deposit minted fewer shares than `max_v_in`, which the next
    /// leg pulls in full. Size `max_v_in` below the shares the deposit mints.
    DepositDidNotFundMaxVIn = 5,
    /// The vault took more of the base asset than `max_asset_in`.
    AssetSpentOverMax = 6,
    /// Base asset delivered is below `min_asset_out`.
    MinAssetOutNotMet = 7,
    /// Vault shares delivered are below `min_shares_out`.
    MinSharesOutNotMet = 8,
    /// LP shares minted are below `min_lp_out`.
    MinLpOutNotMet = 9,
    /// The zap left more vault shares than `sweep_allowance` lets the router
    /// redeem.
    SweepAllowanceTooLow = 10,
    /// The swap delivered less PT than `pt_to_buy`.
    PtLegShort = 11,
    /// The user holds less PT than `pt_to_sell`.
    PtBalanceShort = 12,
}
