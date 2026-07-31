# YT Zap Transaction Budget

**Status:** resolved — every zap now simulates within budget against a Blend vault.
Post-maturity paths remain unverified.
**Investigated and fixed:** 2026-07-31, Stellar testnet, protocol 27
**Reference market:** `1817200000` (see Appendix)

---

## 1. Summary

Both asset-denominated YT zaps used to fail with `HostError: Error(Budget, ExceededLimit)`.
The previous analysis concluded this was **structural** — that a flash swap inherently cost more
than a vault deposit plus a spot swap, so no optimisation could close the gap, and YT would
permanently be a two-transaction product.

That was wrong on two independent counts.

1. **The market it was measured against was frozen on stale bytecode.** Markets are immutable and
   `create_market` stamps whatever hashes the factory holds at creation. Market `1789517001` was
   created two days before the yield-token and yield-manager fixes were installed, so it
   permanently runs the pre-fix contracts. On a market built from current WASM,
   `zap_yt_for_asset` succeeds untouched.
2. **The binding resource is memory, not CPU.** Every measured path sits between 13M and 32M
   instructions against a 100M limit, so the failure was never CPU exhaustion. By elimination it
   is the 40 MB memory budget, dominated by how many times the Blend pool's reserve record is
   materialised as host objects.

The remaining failure, `zap_asset_for_yt`, was fixed by removing a redundant vault read: the AMM
now hands the rate it already loaded to the flash callback instead of the yield manager fetching
it again. See §3.

**All eight zaps and all four share-denominated swaps now pass.** §4 has the numbers.

---

## 2. What was wrong

### Root cause A — markets are frozen on the WASM current at creation

| contract | market `1789517001` | current at the time |
|---|---|---|
| AMM | `f910f83a…` | `f910f83a…` ✅ |
| yield token | `6b0004f5…` | `18bcafd9…` ❌ |
| yield manager | `4ebac6b1…` | `067d8228…` ❌ |

The market was created 2026-07-27 21:42, two minutes after a new AMM was installed and two days
before the YT/YM fixes. The stale yield token looked the rate up **twice** per transfer — visible
directly in the failing trace as two `get_exchange_rate` calls — which the current one fixes.

Every "the YT zaps are structurally impossible" measurement was taken against that market. See
`ARCHITECTURE.md` §4.10 for the general hazard.

### Root cause B — the binding limit is memory

`Error(Budget, ExceededLimit)` covers CPU (100M instructions) and memory (40 MB). CPU is excluded
by arithmetic: `zap_yt_for_asset` **succeeds** at ~31M instructions, and `zap_asset_for_yt`
differs from it by one vault deposit. For CPU to bind, that deposit would have to cost more than
68M — yet `zap_asset_for_pt`, which contains a deposit *and* a spot swap *and* a redeem, totals
under 25M.

This reframes the optimisation target. Memory is not driven by arithmetic but by host-object
allocation, and the largest single object on these paths is Blend's `get_reserve` return value —
a ~20-field record materialised in full on **every** vault rate read.

---

## 3. The fix

The AMM loads the vault rate at the top of every flash swap to price the trade. The yield manager,
called back moments later inside that same flash swap, used to fetch the identical value again —
same call, same argument (`FP_SCALE` and `SCALAR_7` are both `1e7`; verified live as byte-identical
returns). The rate cannot move mid-transaction, so the second read spent a full Blend round trip
recomputing a number the caller was already holding.

**What changed:**

* `amm-interface` — both flash-receiver callbacks gained a `vault_rate` parameter. `AmmInterface`
  is untouched, so the router needed no change.
* `amm/src/vault.rs` — `VaultRate` exposes the rate it already probed.
* `amm/src/contract.rs` — both flash swaps pass it down.
* `yield_manager` — `update_exchange_rate` split so the vault read is separable from the policy.
  The non-decreasing floor, the maturity lock and the storage write all stay in the yield manager;
  only the cross-contract read moves.

**Why it is safe** (documented at `update_exchange_rate_from`):

* `update_exchange_rate_from` is a private Rust function in a non-`#[contractimpl]` block — it is
  not in the contract ABI and cannot be invoked externally.
* The only external door is the callback, gated by `get_pool(&env).require_auth()`. For a contract
  address that is satisfiable only by invoker auth, so a direct caller cannot forge it.
* `set_pool` is one-shot, so the registered pool cannot be re-pointed at an attacker's contract.
* Routing through the real pool offers no injection point: the rate is not a `flash_swap_*`
  parameter, and the AMM asserts `receiver == get_ym(&e)`.
* The non-decreasing floor discards a supplied value below the stored rate, so only an inflated
  one could do harm — and that requires the immutable, factory-deployed AMM to misreport.

**Direction is the load-bearing property.** The value flows from the contract that read the vault
*directly* to the contract that applies policy on top of it. The reverse — sourcing the AMM's
pricing rate from the yield manager — was considered and rejected: the YM's rate is
high-water-marked, so the AMM would price its reserve off a figure that can be stale-high.

**One new invariant.** The yield manager now depends on the caller's vault being its own vault.
Guaranteed by construction — `create_market` threads one `vault` value into both, and `set_pool`
is one-shot — so no runtime check was added. Before this parameter existed the YM was
self-consistent whatever the AMM referenced, so anything that ever lets the two be deployed apart
must re-establish it.

**Not a deviation from SEP-56.** The vault interface (`vault/vault_interface`) is untouched. The
protocol calls the same four functions, just fewer times, so third-party vaults benefit too — a
vault with an expensive rate call benefits most. This is the property a vault-side memo would have
given up.

---

## 4. Results

Same market shape, same trade sizes, before and after.

| entrypoint | before R2 | after R2 | reads |
|---|---|---|---|
| **`zap_asset_for_yt`** | **exceeded** | **32,136,043 (32.1%)** | 2 → **1** |
| `zap_yt_for_asset` | 31,462,249 | 29,873,810 (29.9%) | 3 → 2 |
| `swap_v_for_yt` | 23,027,555 | 21,403,993 (21.4%) | 2 → 1 |
| `swap_yt_for_v` | 24,926,368 | 23,312,984 (23.3%) | 3 → 2 |
| `zap_asset_for_lp` | 30,513,346 | 30,498,817 (30.5%) | 1 |
| `zap_asset_for_pt` | 24,815,719 | 24,808,698 (24.8%) | 1 |
| `zap_lp_for_asset` | 23,579,390 | 23,587,876 (23.6%) | 1 |
| `zap_pt_for_asset` | 19,866,997 | 19,880,824 (19.9%) | 1 |
| `zap_asset_for_split` | 17,365,034 | 17,417,349 (17.4%) | 1 |
| `zap_split_for_asset` | 16,947,860 | 16,991,089 (17.0%) | 1 |
| `swap_v_for_pt` | 14,113,860 | 14,110,653 (14.1%) | 1 |
| `swap_pt_for_v` | 13,387,537 | 13,399,031 (13.4%) | 1 |

Every flash-swap path dropped one vault read and ~1.6M instructions. Non-flash paths are unchanged
within ±0.1%, as expected.

**The saving was smaller than predicted yet sufficient.** The estimate was 2–4M against a ~5M gap —
a coin flip. The actual instruction saving is ~1.6M, *less* than the low end, and it was still
enough. That is direct evidence that instruction counts understate the memory relief of removing a
Blend read, consistent with §2's conclusion that reserve-record materialisation dominates memory.

### Per-leg cost model

Derived by decomposition; useful for costing a proposed path before building it.

| leg | instructions |
|---|---|
| vault deposit (Blend submission) | ~5.0M |
| sweep (`query_asset` + `approve` + `redeem`) | ~6.5M |
| AMM spot swap | ~13.4M |
| AMM flash swap | ~25M |

⚠️ These are instruction counts standing in for a memory-bound limit. Use the model to **rank**
levers, never to predict the ceiling — as §4 shows, it understates what removing a vault read buys.

---

## 5. How to measure

All of this is simulation-only (`--send=no`): no fees, no state change.

```bash
stellar contract invoke --very-verbose --send=no --id <router> ... 2>&1 \
  | grep -o 'transaction_data: "[^"]*"'
# then decode — the RPC zeroes cost.cpu_insns, but transaction_data carries the real figures
stellar xdr decode --type SorobanTransactionData --input single-base64 --output json
# → resources.instructions, resources.write_bytes, resources.footprint
```

**Counting vault reads:** count `convert_to_assets` occurrences in the `--very-verbose` output and
halve it (each call logs an `fn_call` and an `fn_return`).

**Locating a failure inside a call chain:** use an existing assert as a tripwire. `sweep_gained_shares`
asserts `gained <= sweep_allowance` immediately *before* the vault redeem, so passing
`sweep_allowance = 1` distinguishes them — `Error(Budget, ExceededLimit)` means the budget blew
before the sweep, `UnreachableCodeReached` means everything up to the sweep completed.

**Checking what a market actually runs** (not what the registry says was installed):

```bash
stellar contract fetch --id <ym> --network testnet --out-file ym.wasm && sha256sum ym.wasm
```

---

## 6. Remaining improvements

Ordered by value. None are blocking; every zap works today.

### I1 — Verify the post-maturity paths *(highest value)*

`exit_expired` and `exit_expired_to_asset` have **never executed on-chain against R2 contracts**.
Every current market matures in 2027, so both revert on the maturity assert. This matters because
`exit_expired_to_asset` is the heaviest path in the system — LP withdrawal, full PT redemption, YT
yield claim and a vault redeem in one transaction — and the yield manager has just changed
underneath it.

The factory only enforces `maturity > now`, with no minimum. So create a market dated ~10 minutes
out, seed it, wait, and exercise both exits. That closes the last unverified surface for the cost
of one short-lived market.

### I2 — Remove the buy side's second Blend submission

`zap_asset_for_yt` is now the heaviest path at 32.1%. It is the only one carrying **two** Blend
submissions plus a flash swap: a deposit inbound, and a redeem to hand back the change. The second
exists only because `max_asset_in` must be an over-estimate — Soroban matches signed auth arguments
exactly, so the deposit amount is fixed before execution and cannot be "exactly what the trade
needs". Worth ~6.5M.

Four approaches, none free:

* **(a) YM deposits with itself as receiver**, mirroring `zap_asset_for_split`, so no shares ever
  reach the user. Removes the user-side refund but not the deposit overshoot.
* **(b) Exact-cost pull.** Replace "pull `max_v_in`, refund the excess" with "user pre-approves a
  ceiling, YM `transfer_from`s the measured cost" — the pattern `exit_expired_to_asset` already
  uses. Eliminates the refund; the deposit overshoot remains.
* **(c) Make the sweep optional.** A frontend that has sized `max_asset_in` tightly could skip it.
  Cheapest to implement, but it trades away the "a zap leaves your share balance where it found
  it" guarantee — it would have to become an explicit caller choice, not a silent default.
* **(d) Route dust leftovers to the surplus counter** that `collect_surplus` already sweeps,
  instead of paying for a redeem. Defensible only for genuine dust, and it is a value transfer
  from user to protocol, so the threshold needs justifying.

### I3 — The sell side's last duplicate read is probably irreducible

`zap_yt_for_asset` still makes two vault reads: one from the yield manager (the YT transfer's
`accrue_yield`) and one from the AMM (`VaultRate::load`). They are in **different contracts**, so
neither R2 nor a YM-side memo can merge them, and the YT transfer happens *before* the flash swap
by design — moving it later would put a pool-priced amount into the user's signed auth entry.

Closing it would need either a vault-side per-ledger memo (which adds an unwritten "cheap repeated
reads" requirement for third-party vaults — see `ARCHITECTURE.md` §4.9) or a restructure that
defeats signability. Recorded as understood rather than open.

### I4 — Make on-chain failures distinguishable

The release profile sets `panic = "abort"` and `strip = "symbols"`, so **every** assert failure
surfaces on-chain as an identical `UnreachableCodeReached`. "min_asset_out not satisfied", "market
not expired" and "sweep_allowance below the shares this zap produced" are indistinguishable to a
frontend, and were indistinguishable during this investigation — the failure point had to be
inferred by probing.

The yield manager already uses a `contracterror` enum (`YieldManagerError`). The router uses bare
`assert!` throughout. Converting them would give frontends real error codes at the cost of some
WASM size.

### I5 — Get a real memory measurement

§2's conclusion is a deduction: the RPC returns `cost.mem_bytes: 0`, so no tool here reports the
actual figure. The argument survives wide error bars, but it is inference. A local host with a
Blend fixture would settle it and would let the budget tests actually guard the binding resource.
Blocked on `tests/blend` being excluded pending a `blend-contract-sdk` compatible with
soroban-sdk 26.

### I6 — Watch the headroom

`zap_asset_for_yt` (32.1%) and `zap_asset_for_lp` (30.5%) are the closest to whatever the true
ceiling is — and `zap_asset_for_yt` failed at an estimated ~36% before R2. The practical margin is
therefore thin, and instruction percentage is **not** headroom. Re-measure after any change that
adds a vault touch, and before mainnet.

### I7 — Check in a testnet smoke script

The integration suite cannot catch this class of problem (§7), so the only real guard is simulating
every entrypoint against a live market and reading the resource numbers back. That was done by
hand here; turning it into a checked-in script — invoke each entrypoint with `--send=no`, decode
`transaction_data`, print instructions and read counts — makes it repeatable and gives a
regression baseline.

### I8 — Tooling for market/WASM drift

`ARCHITECTURE.md` §4.10 documents the hazard and markets now carry a `deployed_wasm` block, but
nothing detects drift automatically. A small tool that fetches a market's contracts, hashes them,
and diffs against `factory.get_wasm_hashes()` would have caught root cause A immediately.

---

## 7. Why the test suite cannot catch this

`tests/integration/src/tests/zaps.rs` asserts CPU and memory against the network limits and all
tests pass. It cannot detect this class of failure, for three structural reasons:

1. **The router and vault are registered natively, not as WASM** — precisely the two components a
   zap adds work to. The file says so itself (`zaps.rs:291`: *"judge by headroom, not by the
   assert"*).
2. **The vault under test is OpenZeppelin's, not Blend.** No lending pool behind it, so a rate read
   is a storage load rather than a reserve materialisation with interest accrual. The dominant
   memory consumer does not exist in the harness.
3. **Stale bytecode is unrepresentable.** Every test registers freshly built contracts, so a market
   frozen on a two-day-old yield token cannot be modelled.

The suite is not wrong; it measures a different system. This class of issue has now surfaced on
testnet twice.

---

## 8. Not verified

* **Memory as the binding limit is a deduction, not a reading** (§2, I5).
* **Post-maturity paths have never run on R2 contracts** (I1).
* **Improvements I2–I8 are reasoned, not implemented.**
* Measurements come from a thinly seeded pool (1e9 PT / 6.24e8 V) with 10-unit trades. Fixed-point
  arithmetic does not cost more for larger numbers, but markets with more accumulated state may.

---

## Appendix — addresses

**Shared infrastructure**

| | |
|---|---|
| factory | `CCEPZTQWAHIBQVZEDGI6J7D3FVNJIZPUDHCORMOGVA3HZAIRVQTY2V5L` |
| router | `CBXG7TKSE5TD7NLAT2MU7CCGC2XN34PNANJZBT6M2SEIOZWZ5DPVK4HV` |
| blend vault wrapper | `CCWNH24WDHWW6U7LPZ3K2TFLF7IVOKGN6GQMJXUCTYV6Z7AQ6BX7FYGB` |
| blend pool | `CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF` |
| XLM (underlying) | `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC` |

**Reference market `1817200000`** — R2 contracts, all entrypoints verified

| | |
|---|---|
| ym | `CBGILN3IHQPYCEVF7AABRT7AXBS2SZB7MLYDXB63R44VXLQVR3BWBYV5` |
| pt | `CBOH24SHAPJJ6ENSSDYBL6C5AHTWZ4LMT7HR5QAOAQIMNNK4KQIHGOLO` |
| yt | `CBOG5GXPCNNVLTID6DNCRPZBI56Q6ZJ57O2R65HXY6QJDAZWXZJKHMTQ` |
| pool | `CAFX5YV5E2ZC5PQGQK6UUKYF7ZL3VRSG2W3OZPJMJG7PH7J3JTSLAOET` |

**Baseline market `1817100000`** — pre-R2, kept for comparison

| | |
|---|---|
| ym | `CDQUUMSJX3ECMY4DNWGJ743RM67SFGSL5CXQIAYCUXVIRJTQLRA4LP2E` |
| pool | `CCKLJN7WEYTWI63WJT5C76XSDJ4P5XSBHDMUPNSRFFDFXRELVJSOBA64` |

**Stale market `1789517001`** — frozen on pre-fix YT/YM; both YT zaps and `exit_expired_to_asset`
are broken on it. Matures 2026-09-16.
