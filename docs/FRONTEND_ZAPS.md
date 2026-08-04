# Zaps — Frontend Integration Guide

Everything a client needs to call the eight base-asset zaps plus
`exit_expired_to_asset` on the router.

**Audience:** whoever builds the trade UI. No Rust required.
**Contract source:** `contracts/router/src/contract.rs`
**Background:** `docs/ARCHITECTURE.md` §4.8, `docs/YT_ZAP_BUDGET.md`

---

## 1. What a zap is

The protocol is denominated internally in **V** — vault shares (e.g. a Blend
XLM position). Users don't want to hold or think about V. A zap wraps the vault
deposit or redeem around the real operation so the user arrives and leaves
holding only the **base asset** (XLM, USDC, whatever the vault's underlying is).

| Zap | What the user does | Path |
|---|---|---|
| `zap_asset_for_pt` | Buy fixed yield | asset → V → PT |
| `zap_pt_for_asset` | Sell PT | PT → V → asset |
| `zap_asset_for_yt` | Buy leveraged yield exposure | asset → V → YT (flash swap) |
| `zap_yt_for_asset` | Sell YT | YT → V → asset |
| `zap_asset_for_split` | Mint PT+YT together | asset → V → PT+YT |
| `zap_split_for_asset` | Recombine PT+YT | PT+YT → V → asset |
| `zap_asset_for_lp` | Provide liquidity | asset → V → LP |
| `zap_lp_for_asset` | Remove liquidity | LP → V → asset |
| `exit_expired_to_asset` | Close everything after maturity | LP+PT+YT → asset |

All of them are **one transaction, one signature**. The router holds no funds at
any point — every leg names the user as the token-holding party, so a revert
anywhere unwinds the whole thing and strands nothing.

---

## 2. The one rule that explains every weird parameter

> **Nothing the chain computes may appear in an argument the user signs.**

Soroban matches a signed authorization **argument-for-argument** at execution
time. Your wallet builds that signature by simulating first; the transaction
executes a ledger or more later. So any argument the chain computes in between
— a vault share count, a pool-priced amount, `ledger().sequence()` — is a
mismatch waiting to happen. Against a yield-bearing vault whose rate accrues
every ledger, it's a *guaranteed* one.

This is not theoretical. The first on-chain zap attempt failed instantly with
`Auth/InvalidAction` because the router called
`approve(user, ym, <measured shares>, <current ledger>)` — both execution-time
values.

Three consequences you will feel:

**(a) Every parameter is caller-chosen.** You supply bounds, not exact amounts.
There is no "let the contract figure out how much." That's why the signatures
are long.

**(b) Bounds double as funding.** A `max_*` bound is *pulled in full* and the
excess refunded. So the user's account must actually hold `max_asset_in`, not
just the expected spend. **Balance-check against the bound, not the estimate.**

**(c) Leftovers move under contract authority, via a pre-signed ceiling.**
Anything measured mid-transaction (leftover shares, the LP payout) is converted
by the router acting as vault operator against a `sweep_allowance` /
`sweep_expiry` allowance the user signed up front. Hence those two parameters
on most zaps.

`zap_auth_entries.rs` in the integration suite pins the exact auth tree for
three representative paths under real signature-matching rules. Read it if a
signature is being rejected.

---

## 3. Common parameters

Every zap starts with the same four:

| Param | Type | Meaning |
|---|---|---|
| `vault` | `Address` | Vault contract. Identifies the market together with `maturity`. |
| `maturity` | `u64` | Unix seconds. The router resolves `(vault, maturity)` through the factory, so you cannot point it at a pool the factory didn't deploy. |
| `to` | `Address` | The user. Payer **and** recipient — `to.require_auth()`. You cannot zap on behalf of someone else. |
| …operation params… | `i128` | See per-function sections. |

### `sweep_allowance` / `sweep_expiry`

Present on all zaps **except** the two split zaps (which need no sweep — the YM
deposits with itself as receiver, so shares never touch the user's account).

* `sweep_allowance: i128` — ceiling on vault shares the router may redeem back
  to the asset. Must be **≥ the shares the zap actually leaves over**, or the
  call reverts. Unused allowance is never taken.
* `sweep_expiry: u32` — **ledger sequence** at which the allowance dies.

**How to pick them:**

```ts
const { sequence } = await rpc.getLatestLedger();
const sweepExpiry = sequence + 200;          // ~15-20 min at ~5s/ledger
const sweepAllowance = expectedLeftoverShares * 3n; // generous; over-providing is free
```

Rules:

1. **Compute `sweep_expiry` client-side, once, before signing.** Never let it
   be re-derived. It must be ≥ the ledger the tx actually executes at, and
   within the network's max entry TTL for a temporary entry.
2. **Don't be stingy with `sweep_allowance`** — too low reverts the whole
   transaction ("sweep_allowance below the shares this zap produced").
3. **Don't be reckless with `sweep_expiry`** — the `approve` sets the allowance
   to the full `sweep_allowance`, and only the measured leftover is consumed.
   The remainder stays live until expiry. Minutes, not days.
4. **`exit_expired_to_asset` is the exception — see §5.9.** There the allowance
   goes to the yield manager and covers the user's *entire* share balance.

---

## 4. There are no quote functions — you must simulate

The AMM exposes `get_reserves`, `get_implied_rate`, `get_total_shares`,
`balance_shares`. Those are fine for display. They are **not** enough to size a
trade: the curve math is non-trivial and duplicating it client-side would drift
from the contract.

The intended flow is:

```
1. simulate the real invocation with deliberately wide bounds
2. read the return value (the actual cost / proceeds)
3. re-build with real bounds = simulated value ± slippage
4. simulate again to get final resource footprint + auth entries
5. sign and submit
```

Every zap returns an `i128` you can use for step 2:

| Function | Returns |
|---|---|
| `zap_asset_for_pt` | asset actually spent |
| `zap_pt_for_asset` | asset delivered |
| `zap_asset_for_yt` | asset actually spent |
| `zap_yt_for_asset` | asset delivered |
| `zap_asset_for_split` | tokens minted (PT and YT each — they mint in equal measure) |
| `zap_split_for_asset` | asset delivered |
| `zap_asset_for_lp` | LP shares minted |
| `zap_lp_for_asset` | asset delivered |
| `exit_expired_to_asset` | asset delivered |

Read it with `scValToNative(sim.result.retval)`.

### Auth entries

If `to` **equals the transaction source account**, the auth entries come back as
`SOROBAN_CREDENTIALS_SOURCE_ACCOUNT` and the transaction signature covers them —
nothing extra to do. **Keep it that way.** If `to` differs from the source you
must sign each auth entry separately (`authorizeEntry` + the wallet's
`signAuthEntry`), which is more surface area for exactly the drift this design
avoids.

**Never change an argument between the final simulation and submission.** Not
the bounds, not the expiry, not by a stroop.

---

## 5. Per-function reference

All amounts are `i128` in the token's native precision (7 decimals on the
testnet XLM market, so `100_000_000` = 10.0 XLM).

### 5.1 `zap_asset_for_pt` — buy fixed yield

```
zap_asset_for_pt(vault, maturity, to, pt_out, max_asset_in, max_v_in,
                 sweep_allowance, sweep_expiry) -> i128
```

Buys **exactly** `pt_out` PT. `max_asset_in` is deposited in full, `max_v_in` is
handed to the pool, the pool keeps only what the trade costs, and everything
left over is swept back to the asset.

| Param | How to pick |
|---|---|
| `pt_out` | The user's target PT amount. Exact — this is what they get. Must be > 0. |
| `max_v_in` | Simulated share cost × (1 + slippage). Must be > 0. |
| `max_asset_in` | Assets needed to mint `max_v_in` shares, × ~1.1. Must be > 0. |

There is a window both bounds must sit inside:

```
trade cost in shares  ≤  max_v_in  ≤  shares that max_asset_in mints
```

The lower edge is pool slippage; the upper edge is enforced by the assert
`"deposit did not fund max_v_in"`. Note the direction of the vault-rate risk:
shares **appreciate**, so a given asset amount mints *fewer* shares as time
passes. Size `max_v_in` conservatively **below** your simulated share count, and
pad `max_asset_in` above it.

**User must hold `max_asset_in` of the base asset.** Refund comes back in the
same transaction.

**Returns** asset actually spent. Asserted `≤ max_asset_in`.

---

### 5.2 `zap_pt_for_asset` — sell PT

```
zap_pt_for_asset(vault, maturity, to, pt_in, min_asset_out,
                 sweep_allowance, sweep_expiry) -> i128
```

Sells exactly `pt_in` PT and leaves the user holding the base asset.

| Param | How to pick |
|---|---|
| `pt_in` | Exact PT to sell. Must be > 0. |
| `min_asset_out` | Simulated proceeds × (1 − slippage). **Must be > 0** — you cannot pass 0 to disable the check. |

The pool leg internally uses a share bound of `1` (the widest legal value); the
real slippage protection is `min_asset_out`, denominated in the asset. That
single number covers pool price *and* vault rate together, which is the only
figure the user cares about.

**Returns** asset delivered. Asserted `≥ min_asset_out`.

---

### 5.3 `zap_asset_for_yt` — buy YT

```
zap_asset_for_yt(vault, maturity, to, yt_out, max_asset_in, max_v_in,
                 sweep_allowance, sweep_expiry) -> i128
```

Same shape as `zap_asset_for_pt`, same sizing rules, same funding requirement.
Internally it's a **flash swap** through the yield manager rather than a spot
swap, which makes it the most expensive path in the system (see §7).

YT costs only the yield portion, so the spend is far below `yt_out` face
amount — a UI that sizes `max_asset_in` off the face amount will be wildly
over-provisioned. Simulate.

Emits `routed_yt_buy`.

---

### 5.4 `zap_yt_for_asset` — sell YT

```
zap_yt_for_asset(vault, maturity, to, yt_in, min_asset_out,
                 sweep_allowance, sweep_expiry) -> i128
```

Same shape as `zap_pt_for_asset`. `yt_in > 0`, `min_asset_out > 0`.

Emits `routed_yt_sell`.

---

### 5.5 `zap_asset_for_split` — mint PT + YT

```
zap_asset_for_split(vault, maturity, to, asset_in, min_tokens_out) -> i128
```

**The simplest zap. No sweep parameters, no bounds gymnastics.** Deposits
`asset_in` and mints PT and YT in equal measure. Touches no AMM, so no price
impact and no swap fee — the only loss is rounding.

| Param | How to pick |
|---|---|
| `asset_in` | Exact asset to spend. Must be > 0. |
| `min_tokens_out` | Simulated mint × (1 − small margin). Guards the vault rate only. |

The yield manager deposits into the vault with **itself** as receiver, so shares
never pass through the user's account — nothing to sweep, no allowance to grant.
This is why it signs the smallest tree of any zap.

**Returns** the amount of each token minted (PT and YT are equal).

---

### 5.6 `zap_split_for_asset` — recombine PT + YT

```
zap_split_for_asset(vault, maturity, to, amount, min_asset_out) -> i128
```

Burns `amount` of **both** PT and YT and returns the underlying. Pre-maturity
only. Near-lossless (rounding only).

Requires the user to hold `amount` of each. `amount > 0`, `min_asset_out > 0`.

**Returns** asset delivered.

---

### 5.7 `zap_asset_for_lp` — provide liquidity

```
zap_asset_for_lp(vault, maturity, to, asset_in, pt_to_buy, max_v_in,
                 desired_v, min_lp_out, sweep_allowance, sweep_expiry) -> i128
```

The most parameter-heavy zap, and the one that needs the most from the frontend.

Deposits `asset_in` into the vault, buys `pt_to_buy` PT with part of the
proceeds, then adds both legs to the pool.

| Param | How to pick |
|---|---|
| `asset_in` | Total asset the user is committing. Must be > 0. |
| `pt_to_buy` | **Your job.** The PT half of the split. May be 0 (skips the swap entirely — pure V-side deposit). |
| `max_v_in` | Share bound for the PT purchase. Only checked when `pt_to_buy > 0`. |
| `desired_v` | The share leg offered to the pool. Must be > 0. |
| `min_lp_out` | The real slippage bound. Must be > 0. |

**Why `pt_to_buy` is yours to compute:** the pool only accepts its two legs in
the current reserve ratio. Solving for the split that lands exactly on that
ratio means duplicating the AMM's curve math in the router, which would drift
from it. Instead you simulate for the figure and the router refunds whatever the
pool declines. **A slightly wrong number costs a small refund, not a failed
transaction.** Start from `get_reserves()` for the ratio, refine by simulation.

`desired_v` is the same deal — offered in full, the pool takes what its ratio
allows and refunds the rest. Per-leg minimums are deliberately 0; `min_lp_out`
is the bound that matters.

**Leftover handling differs by token:**
* Leftover **shares** → swept back to the asset.
* Leftover **PT** → **stays with the user.** Selling a dust amount back into the
  pool can trip the AMM's positive-amount asserts and revert an otherwise-good
  transaction, and PT is a token the user may want to keep anyway. **Your UI
  should show this residual PT** — users will otherwise be confused by a
  balance they didn't ask for.

**Returns** LP shares minted.

---

### 5.8 `zap_lp_for_asset` — remove liquidity

```
zap_lp_for_asset(vault, maturity, to, lp_shares, pt_to_sell, min_asset_out,
                 sweep_allowance, sweep_expiry) -> i128
```

Burns `lp_shares`, sells the PT leg back into the pool, redeems the whole
proceeds through the vault.

| Param | How to pick |
|---|---|
| `lp_shares` | LP to burn. Must be > 0. |
| `pt_to_sell` | **Your figure** — typically what you simulated the withdrawal will yield. May be 0. |
| `min_asset_out` | Simulated proceeds × (1 − slippage). Must be > 0. |

`pt_to_sell` cannot be measured on-chain and then sold — the measured amount
would land in the user's signature. Consequences:

* Anything the withdrawal produces **beyond** `pt_to_sell` stays with the user.
* The caller may deliberately include PT they **already held** — they're
  stating intent, not having it inferred. This is a legitimate UI feature
  ("also sell my loose PT"), not a bug.

⚠️ **Warn the user on large exits.** Selling the PT leg into the same pool it
just came out of moves the price against the seller, and the effect grows with
position size — a large exit realises noticeably less than the position's quoted
value. For an **expired** market use `exit_expired_to_asset` instead, where PT
redeems at par through the YM and no swap is needed.

**Returns** asset delivered.

---

### 5.9 `exit_expired_to_asset` — close everything post-maturity

```
exit_expired_to_asset(vault, maturity, to, lp_shares, max_pt, pt_allow_expiry,
                      min_asset_out, sweep_allowance, sweep_expiry) -> i128
```

One signature takes an expired position — LP, all PT, and any accrued YT yield —
all the way back to the base asset.

| Param | How to pick |
|---|---|
| `lp_shares` | LP to unwind. May be 0 (PT/YT only). |
| `max_pt` | Ceiling on PT to redeem. Must be > 0. Set **generously**. |
| `pt_allow_expiry` | Ledger sequence for the PT allowance. Same rules as `sweep_expiry`. |
| `min_asset_out` | Must be > 0. |
| `sweep_allowance` | Must be > 0. **Read the warning below.** |

Reverts unless `ledger.timestamp() >= maturity`.

`max_pt` is a ceiling granted to the YM as an allowance. The YM burns
`min(max_pt, balance)`, so "redeem everything, including the PT this withdrawal
just produced" works without the measured figure entering the signature. Unused
allowance is never taken and dies at the expiry.

> ### ⚠️ `sweep_allowance` here is different
>
> On the other zaps the sweep covers the leftover the zap produced. Here the
> allowance goes to the **yield manager**, and it converts the caller's
> **entire vault-share balance** up to `sweep_allowance` — including shares held
> for **unrelated markets**. This has been observed live sweeping unrelated
> leftover shares.
>
> The YM authenticates `from`, so the figure it takes must be a pre-signed
> ceiling rather than a delta measured mid-call. There is no way around it.
>
> **Size `sweep_allowance` to the expected proceeds of this exit.** Do not pass
> a large round number. If the user holds shares for other markets, this is a
> real footgun and your UI should size it tightly.

**Returns** asset delivered. Emits `exit_expired_to_asset`.

> **Status:** neither expired-exit path has ever executed on-chain against the
> current (R2) contracts — every live market matures in 2027, so both revert on
> the maturity assert. Treat this entrypoint as unproven and test against a
> short-dated market before shipping. See `docs/YT_ZAP_BUDGET.md` §6 I1.

---

## 6. Error handling — read this before you write a catch block

The release profile sets `panic = "abort"` and `strip = "symbols"`. **Every**
assert failure in the router surfaces on-chain as an identical
`UnreachableCodeReached`. "min_asset_out not satisfied", "market not expired"
and "sweep_allowance below the shares this zap produced" are indistinguishable
to your client. (Tracked as `YT_ZAP_BUDGET.md` §6 I4 — converting the router to
a `contracterror` enum would give you real codes; the yield manager already has
one.)

So: **simulate before every submit, and map failures by context yourself.**

| Symptom | Likely cause | Fix |
|---|---|---|
| `UnreachableCodeReached` on an entry zap | `max_v_in` exceeds what `max_asset_in` mints, or pool slippage exceeded `max_v_in` | Widen `max_asset_in`, re-simulate |
| `UnreachableCodeReached` on an exit zap | `min_asset_out` not met | Widen slippage tolerance |
| `UnreachableCodeReached` late in any sweeping zap | `sweep_allowance` below the leftover | Raise `sweep_allowance` |
| `UnreachableCodeReached` on `exit_expired_to_asset` | Market not matured | Check `maturity` vs. now |
| `Auth/InvalidAction` | An argument changed between simulation and submission | See §2 — nothing may be recomputed |
| `Budget, ExceededLimit` | Path over the transaction budget | See §7 |
| Panic before anything moves | A zero or negative amount | All amounts assert `> 0`; `pt_to_buy`, `pt_to_sell` and `lp_shares` (on the expired exit) allow `0` |

**Diagnostic trick** (from the budget investigation): `sweep_gained_shares`
asserts `gained <= sweep_allowance` immediately *before* the vault redeem. Pass
`sweep_allowance = 1` to bisect a failure — `Budget/ExceededLimit` means the
budget blew before the sweep, `UnreachableCodeReached` means everything up to
the sweep completed.

---

## 7. Transaction budget

All twelve entrypoints simulate within budget on the current contracts. Measured
on market `1817200000`, pool seeded 1e9 PT / 6.24e8 V, 10-unit trades:

| Entrypoint | Instructions | % of 100M limit |
|---|---|---|
| `zap_asset_for_yt` | 32,136,043 | 32.1% |
| `zap_asset_for_lp` | 30,498,817 | 30.5% |
| `zap_yt_for_asset` | 29,873,810 | 29.9% |
| `zap_asset_for_pt` | 24,808,698 | 24.8% |
| `zap_lp_for_asset` | 23,587,876 | 23.6% |
| `zap_pt_for_asset` | 19,880,824 | 19.9% |
| `zap_asset_for_split` | 17,417,349 | 17.4% |
| `zap_split_for_asset` | 16,991,089 | 17.0% |

⚠️ **Instruction percentage is NOT headroom.** The binding resource is the 40 MB
memory budget, not CPU — `zap_asset_for_yt` failed at an estimated ~36%
instructions before it was optimised. The RPC returns `cost.mem_bytes: 0`, so
you cannot see the real constraint.

Practical implications for the client:

* **Always simulate.** A path that works at 10 units may not at 10,000 against a
  market with more accumulated state.
* `zap_asset_for_yt` and `zap_asset_for_lp` are the thin-margin paths. If
  anything is going to blow up, it's those two.
* Surface `Budget/ExceededLimit` as its own error state with a "try a smaller
  amount" suggestion, not as a generic failure.

---

## 8. Market discovery and the stale-WASM hazard

Resolve markets through the factory:

```
factory.get_market(vault, maturity) -> Option<Market>
// Market { name, ym, pt, yt, pool, maturity, vault }
```

The router does this internally on every call, so a caller cannot point it at a
pool the factory didn't deploy.

⚠️ **Markets are immutable and freeze whatever WASM the factory held at
creation.** None of YM/PT/YT/pool expose an upgrade entrypoint, so a market can
never adopt a later version. **A market being live is not sufficient grounds to
surface it in a UI.**

Concretely, on testnet today:

| Market | Status | Note |
|---|---|---|
| `1817200000` | ✅ live | R2 contracts. All twelve entrypoints verified on-chain. **Use this one.** |
| `1817100000` | superseded | Pre-R2 diagnostic baseline. `zap_asset_for_yt` exceeds budget. |
| `1789517001` | ⚠️ **stale** | Frozen on pre-fix YT/YM. **Both YT zaps and `exit_expired_to_asset` are broken here** — the YM doesn't even expose `exit_expired_to_asset`. Share-denominated ops still work. |
| `1789077038` | superseded | Pre-rate-caching AMM. `zap_asset_for_lp` exceeds budget. |

`deployments/deployments.testnet.json` carries a `deployed_wasm` block and a
`verified_onchain` list per market. **Gate which zaps you offer on that list**,
or you'll show users buttons that cannot work. Nothing detects drift
automatically yet (`YT_ZAP_BUDGET.md` §6 I8).

Also: `create_market` is permissionless, and a SEP-56-compliant vault can still
be a bad one — see `ARCHITECTURE.md` §4.9 for the seven-point compatibility
list. Requirement 7 (share value never falls, no exit fees) is **not detectable
on-chain**. Curate which vaults you surface.

---

## 9. Events for indexing

The router publishes at the vault boundary, so an indexer sees every asset
inflow through one event regardless of which zap produced it.

| Topic | Fields (topics marked ᵗ) |
|---|---|
| `zap_in` | `vault`ᵗ, `to`ᵗ, `asset`, `asset_in`, `shares_out` |
| `zap_out` | `vault`ᵗ, `to`ᵗ, `asset`, `shares_in`, `asset_out` |
| `routed_yt_buy` | `vault`ᵗ, `to`ᵗ, `maturity`, `yt_out`, `max_v_in` |
| `routed_yt_sell` | `vault`ᵗ, `to`ᵗ, `maturity`, `yt_in`, `min_v_out` |
| `exit_expired` | `vault`ᵗ, `to`ᵗ, `maturity`, `lp_shares`, `pt_redeemed`, `shares_out` |
| `exit_expired_to_asset` | `vault`ᵗ, `to`ᵗ, `maturity`, `lp_shares`, `asset_out` |

The leg that consumes the shares (AMM swap, YM mint) publishes its own event as
usual — `exit_expired_to_asset` deliberately carries only what the router knows,
with the PT burned and shares redeemed reported in the YM's `RedeemToAsset`.

---

## 10. Testnet addresses

| | |
|---|---|
| factory | `CCEPZTQWAHIBQVZEDGI6J7D3FVNJIZPUDHCORMOGVA3HZAIRVQTY2V5L` |
| **router** | `CBXG7TKSE5TD7NLAT2MU7CCGC2XN34PNANJZBT6M2SEIOZWZ5DPVK4HV` |
| blend vault | `CCWNH24WDHWW6U7LPZ3K2TFLF7IVOKGN6GQMJXUCTYV6Z7AQ6BX7FYGB` |
| XLM (underlying) | `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC` |

**Recommended market `1817200000`** (matures 2027-07-31):

| | |
|---|---|
| ym | `CBGILN3IHQPYCEVF7AABRT7AXBS2SZB7MLYDXB63R44VXLQVR3BWBYV5` |
| pt | `CBOH24SHAPJJ6ENSSDYBL6C5AHTWZ4LMT7HR5QAOAQIMNNK4KQIHGOLO` |
| yt | `CBOG5GXPCNNVLTID6DNCRPZBI56Q6ZJ57O2R65HXY6QJDAZWXZJKHMTQ` |
| pool | `CAFX5YV5E2ZC5PQGQK6UUKYF7ZL3VRSG2W3OZPJMJG7PH7J3JTSLAOET` |

The router has no upgrade entrypoint — it gets a **new address on every
deploy**. Don't hard-code it; read it from the deployments file or make it
configurable.

---

## 11. Checklist

- [ ] `to` == transaction source account (avoids separate auth-entry signing)
- [ ] Simulated with wide bounds, then rebuilt with real bounds
- [ ] Balance check is against `max_asset_in`, **not** the expected spend
- [ ] `sweep_expiry` computed client-side once, buffer of a few hundred ledgers
- [ ] `sweep_allowance` generous on ordinary zaps, **tight** on `exit_expired_to_asset`
- [ ] No `min_asset_out` of 0 anywhere (it will revert)
- [ ] Nothing recomputed between final simulation and submission
- [ ] Market gated on its `verified_onchain` list, not just on existing
- [ ] `Budget/ExceededLimit` handled as its own error state
- [ ] Residual PT after `zap_asset_for_lp` shown in the UI
- [ ] Large `zap_lp_for_asset` exits carry a price-impact warning
