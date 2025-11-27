# YieldBack.Cash (YBC)

A Soroban smart contract protocol for trading interest rate derivatives on Stellar.

## What is YBC?

YBC allows users to split yield-bearing assets (4626-style vault shares) into two separate tokens:

- **Principal Tokens (PT)** - Claim on principal after maturity, earn a fixed interest rate
- **Yield Tokens (YT)** - Earn all variable yield, speculate and bet on interest rates

## How It Works

1. **Deposit** Vault shares (Yield bearing assets) and **receive** PT and YT representing your collateral
2. **Trade** or hold these tokens based on your strategy:
   - Buy/hold PT for predictable, fixed returns
   - Trade YT to speculate on interest rates
3. **Redeem** PT tokens after maturity for the underlying asset after maturity

## Use Cases

### For Fixed Yield Seekers
Principal tokens mimic zero-coupon bonds. Users can redeem a fixed amount at maturity, and users lock in their interest when they purchase the fixed rate principal tokens.

### For Interest Rate Speculators
Yield tokens allow users to bet on interest rates. Users can increase their exposure to interest rate volatility and bet on future interest rates of the underlying protocols (e.g. Blend).

## Development

### Build
``cargo build``

### Test
``cargo test``