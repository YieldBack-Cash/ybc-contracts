# Providing Liquidity — Frontend Integration Guide

Everything a client needs to build an LP flow against the AMM: adding
liquidity, valuing a position, and exiting before or after maturity.

**Audience:** whoever builds the LP UI. No Rust required.
**Contract source:** `contracts/amm/amm/src/contract.rs`, `contracts/router/src/contract.rs`
**Background:** `docs/FRONTEND_ZAPS.md` (§2, §4, §5.7, §5.8, §5.9 in particular),
`docs/ARCHITECTURE.md` §4.6, §5.1

This document assumes `FRONTEND_ZAPS.md` §2 ("nothing the chain computes may
appear in an argument the user signs") and §4 ("there are no quote functions").
Everything here builds on those two rules — read them first.

---

## 1. What an LP position is

The pool holds two reserves:

| | Token | Units |
|---|---|---|
| `reserve_a` | **PT** — principal token | PT, 7 decimals |
| `reserve_b` | **V** — vault shares | vault *shares*, not the underlying asset |

An LP owns a pro-rata claim on both. Shares are tracked by the AMM in its own
storage — see §4, this matters more than it sounds like it should.

The position earns the curve's trading fee (`fee_apy`, capped at 2% — 1% on the
current testnet market) minus the treasury's cut (`reserve_fee_rate`, capped at
50% *of the fee*, 10% on testnet). The LP's share of the fee is left in the
reserves, so it accrues silently into share value. **There is no claim step and
no rewards contract.**

---

## 2. Two entry paths

| Path | Call | Use when |
|---|---|---|
| **Zap** (default) | `Router::zap_asset_for_lp(...)` | User holds only the base asset (XLM, USDC). One transaction, one signature. |
| **Direct** | `Router::deposit(vault, maturity, to, desired_a, min_a, desired_b, min_b)` | User already holds both PT and vault shares. |

Exits:

| Market state | Call |
|---|---|
| Live | `Router::zap_lp_for_asset(...)` → base asset, or `Router::withdraw(...)` → PT + V |
| Expired | `Router::exit_expired_to_asset(...)` → base asset, or `Router::exit_expired(...)` → V |

⚠️ **Never route an expired market through `zap_lp_for_asset`.** After maturity
PT redeems at par through the yield manager and no swap is needed; selling it
into the pool instead just pays price impact for nothing.

Full parameter tables for the zaps live in `FRONTEND_ZAPS.md` §5.7–5.9. This
document covers what those tables don't: how to *choose* the numbers.

---

## 3. The add-liquidity flow

There are no quote functions. `get_reserves`, `get_implied_rate`,
`get_total_shares` and `balance_shares` are for display only — the curve math is
non-trivial and a client-side copy would drift from the contract.

```
1. get_reserves()                → seed a guess for pt_to_buy from the ratio
2. simulate zap_asset_for_lp     → wide bounds: min_lp_out = 1
3. read scValToNative(retval)    → LP shares actually minted
4. rebuild with real bounds      → min_lp_out = simulated × (1 − slippage)
5. simulate again                → final auth entries + resource footprint
6. sign and submit
```

**Never change an argument between the final simulation and submission.** Not
the bounds, not the expiry, not by a stroop. Keep `to` equal to the transaction
source account so auth entries come back as `SOROBAN_CREDENTIALS_SOURCE_ACCOUNT`
and the transaction signature covers them.

### 3.1 Sizing `pt_to_buy` — the actual work

```
zap_asset_for_lp(vault, maturity, to, asset_in, pt_to_buy, max_v_in,
                 desired_v, min_lp_out, sweep_allowance, sweep_expiry) -> i128
```

The zap deposits `asset_in`, buys `pt_to_buy` PT with part of the proceeds, and
offers both legs to the pool. The pool accepts only its current reserve ratio,
so `pt_to_buy` sets the split — and it is yours to compute.

**Seed**, in asset units — `A` = `reserve_a`, `B` = `reserve_b` converted per §5,
`p` = PT price from a swap simulation, `S` = `asset_in`:

```
pt_to_buy ≈ S · A / (B + p·A)
```

**Then iterate.** Buying PT lowers `reserve_a` and raises `reserve_b`, so the
ratio you are aiming at moves as you approach it. Two or three simulations
converge.

* **Must be > 0.** The router passes it through as the pool's `desired_a`, and a
  0 reverts inside `AMM::deposit` with AMM `#21 DepositTooSmall`. The router's
  own check is `>= 0`, so this guard has to be yours.
* **Bias low.** Excess V is swept back to the base asset; excess PT stays in the
  user's wallet, and they paid a swap fee to acquire it.
* **Pre-held PT is never used.** `desired_a` is exactly `pt_to_buy`, bought
  fresh. To contribute PT the user already holds, use `Router::deposit`.
* Leave `min_a` / `min_b` at 0 — `min_lp_out` is the real bound.

### 3.2 `max_v_in` is bounded from above, not just below

```rust
if shares_in < max_v_in {
    return Err(RouterError::DepositDidNotFundMaxVIn); // router #5
}
```

Unlike `max_pt` on `exit_expired_to_asset`, **`max_v_in` cannot be set
generously.** The pool pulls `max_v_in` in full before refunding, so it is
checked against the shares the vault deposit actually produced:

```
max_v_in ≤ shares(asset_in)
```

⚠️ **The vault rate rises every ledger, so the same `asset_in` mints slightly
*fewer* shares at execution than it did in simulation.** A `max_v_in` equal to
the simulated share count will fail with router `#5` a few ledgers later — this
is issue #19, where 5, 10 and 25 XLM zaps failed while 50 and 100 XLM passed,
and where clamping `max_v_in` to the previous simulation's mint still failed.

Size it as the PT leg's cost plus slippage headroom, cap it a margin below the
simulated share count (0.1% is ample), and keep `max_v_in + desired_v`
comfortably under the simulated `shares_in` so the pool deposit still has
shares to pull.

### 3.3 Leftovers differ by token

| Leftover | Fate |
|---|---|
| Vault shares | Swept back to the base asset via `sweep_allowance` / `sweep_expiry` |
| **PT** | **Stays with the user** |

Dust PT is not sold back — a tiny sale can trip the AMM's positive-amount
asserts and revert an otherwise-good transaction, and PT is a token the user may
want to keep. **Show the residual PT in your confirmation screen**, or users
will be confused by a balance they never asked for.

---

## 4. LP shares are not a token

The AMM exposes only `balance_shares(user)` and `get_total_shares()`. There is
**no** `transfer`, no allowance, no SAC, no token interface of any kind. Shares
are an internal ledger entry keyed by address.

Consequences you must design around:

* **Wallets will never show the position.** Your app is the only place it is
  visible. Users who check Freighter after depositing will think the funds
  vanished — say so in the confirmation copy.
* **Positions cannot be sent, staked, or used as collateral.** There is no
  secondary market for them and no "transfer LP" button to build.
* **Portfolio views must poll.** Either call `balance_shares(user)` per market
  (via `Router::balance_shares(vault, maturity, user)`), or index the AMM's
  `deposit` / `withdraw` events. There is no token balance to watch and no
  transfer stream to follow.
* **Total shares includes the dead burn.** `get_total_shares()` counts the 100
  `MINIMUM_LIQUIDITY` shares held by the burn address. Use it as-is for
  pro-rata math — the reserves back those shares too — but don't present it as
  "shares held by users".

---

## 5. Valuing a position

```
share_of_pool = balance_shares(user) / get_total_shares()
your_pt       = reserve_a × share_of_pool     // PT, 7 decimals
your_v        = reserve_b × share_of_pool     // vault SHARES
```

`reserve_b` is denominated in **vault shares**, not the underlying asset.
Convert with the yield manager's rate — assets per 1e7 shares:

```
YieldManager::get_exchange_rate() -> i128   // 1e7-scaled
your_asset_from_v = your_v × rate / 1e7
```

Use the **yield manager's** rate, not the vault's own. PT settles at the YM's
rate and the two diverge the moment a vault loses value; see
`contracts/amm/amm/src/vault.rs` for the argument. Note that
`get_exchange_rate` ratchets its stored value rather than being a pure view, so
read it by simulation like any other call.

### Show two numbers, because they differ

| Figure | How | When it's the right one |
|---|---|---|
| **Value at maturity** | PT counted at par (1 PT = 1 unit of asset) + V at the current rate | The headline for a hold-to-maturity LP |
| **Value if exited now** | Simulate `zap_lp_for_asset` and read the return | The number on the exit button |

The gap between them is real and grows with position size: exiting sells the PT
leg back into the same pool it just came out of, and that moves the price
against the seller. Don't compute the exit figure from reserves — simulate it.

---

## 6. Displaying rates

`get_implied_rate()` returns `last_implied_rate` — the rate **stored at the last
swap**. Deposits don't update it and neither does the passage of time, while the
price a trade actually gets is a function of both the rate *and* the time to
expiry.

* Label it "last traded implied APY", not "current APY".
* **Never quote from it.** Only simulation is authoritative.
* Expect it to look stale on a quiet market. That is correct behaviour, not a
  bug to work around.

The pool has no getter for `fee_apy`, `scalar_root`, or `expiry_ts`. Those come
from the `pool_init` event, the factory's `Market` record, or
`deployments/deployments.testnet.json`.

---

## 7. Guardrails to enforce client-side

**Deposits revert after maturity.** `AMM::deposit` requires
`now < market.expiry_ts` (AMM `#8 MarketExpired`). Withdrawals have no such
check and always work. Show a countdown and disable "Add liquidity" before the
market expires rather than letting the transaction fail.

**Minimum sizes.** The first deposit into an empty pool must mint more than
`MINIMUM_LIQUIDITY` (100 shares, burned to a dead address) or it reverts. Later
deposits revert when floor division rounds the mint to nothing — possible
against a pool with few shares and large reserves. Both surface as AMM
`#21 DepositTooSmall`. Enforce a UI minimum well above both.

**Pool proportion is clamped.** PT/(PT+V) must stay within [1%, 96%]
(`MIN_PROPORTION` / `MAX_PROPORTION` in `curve.rs`, AMM `#13
ProportionOutOfBounds`), and a trade may not push the exchange rate below 1
(AMM `#14 ExchangeRateBelowOne`). Near 96% the `ln(p/(1-p))` term diverges and
PT would price above face value; near 1% truncation would brick the curve.
Surface the current composition so a user looking at a lopsided pool
understands why quotes are behaving strangely.

**Balance-check against the bounds, not the estimate.** `asset_in` and
`desired_v` are pulled in full and refunded. The account must actually hold the
bound.

**`sweep_expiry` is a ledger sequence, not a timestamp.** Compute it client-side
as current sequence + a few hundred ledgers of buffer. `sweep_allowance` can be
generous on `zap_asset_for_lp` and `zap_lp_for_asset` — but must be sized
**tightly** on `exit_expired_to_asset`, where the allowance goes to the yield
manager and it converts the caller's *entire* vault-share balance up to that
ceiling, including shares held for unrelated markets. See `FRONTEND_ZAPS.md`
§5.9.

**`pt_to_sell` on exit is checked against the total PT balance**, not against
what the withdrawal produced. Anything the withdrawal yields beyond `pt_to_sell`
stays with the user; conversely the user may deliberately include PT they
already held. "Also sell my loose PT" is a legitimate checkbox, not a bug.

---

## 8. Errors and budget

Failures come back as `Error(Contract, #n)`. **The code alone is ambiguous:**
router `#5` and AMM `#5` are different errors. Read which contract raised it
from the diagnostic event's `contract:` field (the router address or the pool
address) and look the code up in that contract's table.

This applies to the router deployed 2026-09-13 (`CCUV5QY3…`, see
`deployments/deployments.testnet.json`) and to pools created from AMM wasm
`cee8bc2f…` onward. The previous router and older pools still fail with a bare
`UnreachableCodeReached` for every check.

**Router** (`RouterError`, `contracts/router/src/errors.rs`):

| # | Error | Usual cause on the LP paths |
|---|---|---|
| 1 | `MarketNotFound` | Wrong `vault` / `maturity` |
| 2 | `InvalidAmount` | `asset_in`, `desired_v`, `min_lp_out` ≤ 0; `pt_to_buy` < 0 |
| 3 | `MarketNotExpired` | Expired-exit path called before maturity |
| 4 | `VaultMintedNoShares` | `asset_in` too small to mint a share |
| 5 | `DepositDidNotFundMaxVIn` | `max_v_in` above the shares the deposit minted — §3.2 |
| 6 | `AssetSpentOverMax` | Vault took more than `max_asset_in` |
| 7 | `MinAssetOutNotMet` | Exit slippage bound |
| 8 | `MinSharesOutNotMet` | `exit_expired` slippage bound |
| 9 | `MinLpOutNotMet` | `min_lp_out` slippage bound |
| 10 | `SweepAllowanceTooLow` | `sweep_allowance` below the leftover shares |
| 11 | `PtLegShort` | Swap delivered less PT than `pt_to_buy` |
| 12 | `PtBalanceShort` | `pt_to_sell` above the user's PT balance |

**AMM** (`AmmError`, `contracts/amm/amm-interface/src/lib.rs`):

| # | Error | # | Error |
|---|---|---|---|
| 1 | `ExpiryNotInFuture` | 14 | `ExchangeRateBelowOne` |
| 2 | `InvalidApyBand` | 15 | `TradeTooSmall` |
| 3 | `ApyMaxTooHigh` | 16 | `MaxVInExceeded` |
| 4 | `BandTooNarrow` | 17 | `MinVOutNotMet` |
| 5 | `FeeApyOutOfRange` | 18 | `UntrustedReceiver` |
| 6 | `ReserveFeeRateOutOfRange` | 19 | `FlashSwapNotSettled` |
| 7 | `InvalidAmount` | 20 | `DepositMinNotMet` |
| 8 | `MarketExpired` | 21 | `DepositTooSmall` |
| 9 | `EmptyPool` | 22 | `InsufficientShares` |
| 10 | `ZeroExchangeRate` | 23 | `WithdrawMinNotMet` |
| 11 | `InsufficientPtLiquidity` | 24 | `MathOverflow` |
| 12 | `InsufficientVLiquidity` | 25 | `InvalidPoolState` |
| 13 | `ProportionOutOfBounds` | | |

Codes 1–6 only occur at market creation. Errors from the vault token (e.g.
`#10` insufficient balance on a transfer) and the yield manager carry *their*
contracts' codes the same way.

Still validate preconditions client-side before submitting — a clear message
before signing beats a decoded error after.

`zap_asset_for_lp` is one of the two thin-margin paths (30.5% of the instruction
limit on a small trade). The binding resource is the **40 MB memory budget**,
not CPU, and RPC reports `cost.mem_bytes: 0` — so you cannot see the real
constraint. Always simulate at the user's actual size, and give
`Budget/ExceededLimit` its own error state with a "try a smaller amount"
suggestion rather than folding it into a generic failure.

---

## 9. Events for indexing

From the AMM (`contracts/amm/amm/src/events.rs`):

| Topic | Fields (topics marked ᵗ) |
|---|---|
| `deposit` | `to`ᵗ, `amount_a`, `amount_b`, `shares_minted`, `new_reserve_a`, `new_reserve_b` |
| `withdraw` | `to`ᵗ, `share_amount`, `amount_a`, `amount_b`, `new_reserve_a`, `new_reserve_b` |
| `reserve_fee_paid` | `treasury`ᵗ, `amount` (vault shares) |
| `swap_v_for_pt` / `swap_pt_for_v` | `to`ᵗ, amounts, `new_implied_rate`, `new_reserve_a`, `new_reserve_b`, `fee`, `reserve_fee` |
| `flash_swap_pt` / `flash_swap_v` | `receiver`ᵗ, `user`ᵗ, amounts, `new_implied_rate`, `new_reserve_a`, `new_reserve_b`, `fee`, `reserve_fee` |

`fee` is the whole trading fee and `reserve_fee` the treasury's cut, both in
vault shares; the LPs keep `fee - reserve_fee`. These two trailing fields exist
only on pools created from AMM wasm `cee8bc2f…` onward — decode older pools'
swap events without them.

Every swap event carries the post-trade reserves and implied rate, which is
enough to reconstruct share value over time and compute a realised LP APY
without replaying the curve.

From the router: `zap_in` and `zap_out` at the vault boundary. Full table in
`FRONTEND_ZAPS.md` §9.

---

## 10. Market discovery

Resolve markets through the factory — `get_market(vault, maturity) -> Option<Market>`
— and read addresses from `deployments/deployments.testnet.json` rather than
hard-coding them. The router has no upgrade entrypoint and gets a **new address
on every deploy**.

⚠️ **Markets freeze their WASM at creation** and can never adopt a later version
(`FRONTEND_ZAPS.md` §8). A market existing on-chain is not sufficient grounds to
surface it in a UI — gate on the deployments file's market list, which records
the `amm_wasm_hash` and what has been verified live for each one.
`create_market` is permissionless, so curate which vaults you show;
`ARCHITECTURE.md` §4.9 lists the seven compatibility requirements, one of which
(share value never falls, no exit fees) is not detectable on-chain at all.

---

## 11. Checklist

- [ ] `to` == transaction source account
- [ ] Simulated with wide bounds, rebuilt with real bounds, re-simulated
- [ ] `max_v_in` a margin *below* the simulated `shares_in`, never equal to it
- [ ] `max_v_in + desired_v` comfortably under the simulated `shares_in`
- [ ] Errors decoded by contract address + code, not by code alone
- [ ] `pt_to_buy` > 0 and `min_lp_out` > 0 (either at 0 reverts), per-leg mins at 0
- [ ] Balance check against `asset_in`, not the expected spend
- [ ] `sweep_expiry` is a ledger sequence with buffer, computed once
- [ ] `sweep_allowance` tight on `exit_expired_to_asset`
- [ ] Residual PT after `zap_asset_for_lp` shown to the user
- [ ] Copy explains LP shares won't appear in their wallet
- [ ] Position valued two ways: at maturity, and exit-now (simulated)
- [ ] Implied rate labelled "last traded", never used to quote
- [ ] "Add liquidity" disabled before maturity, exit switched to the expired path
- [ ] Large exits carry a price-impact warning
- [ ] `Budget/ExceededLimit` handled as its own error state
- [ ] Market gated on the deployments file, not just on existing
