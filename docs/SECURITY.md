# YBC Security Model

This document records the trust assumptions, privilege model, and invariants of
the YBC protocol. It is the reference that the code is meant to satisfy: each
invariant below is a property the contracts are designed to preserve, and the
threat notes describe what is and is not defended against.

See [ARCHITECTURE.md](./ARCHITECTURE.md) for how the contracts fit together. The
`§` references below point into that document.

---

## 1. Trust assumptions

| Party / component | What it is trusted for | If it misbehaves |
|-------------------|------------------------|------------------|
| **Vault (4626 share token)** | Reporting an honest, monotonic `convert_to_assets`. The yield manager is the *only* contract that reads it; it high-water-marks the value into the exchange rate that the whole market — mint, redemption, and AMM pricing alike — settles against (§6). | Damage is contained to that one market's opt-in users. A market's YM, PT, YT, and pool touch only its own vault, so a manipulated or broken vault cannot affect any other market. |
| **Factory owner** | Setting the canonical Wasm hashes and the fee config (treasury address + reserve fee rate) snapshotted into new markets, and upgrading the factory contract itself. | Can set bad code or a bad fee config for *future* markets and can upgrade the factory. Cannot reach into already-deployed markets: their YM/PT/YT/pool are separate contracts whose privileged setters are one-shot and already spent, and whose fee config has no setter at all (§3). |
| **Market creator** | Nothing security-critical. Creation is permissionless; the creator is only recorded for off-chain curation. | A malicious creator can spin up a market around a malicious vault, but see the vault row: it only harms users who opt in. |
| **Registered pool (per market)** | Being the sole address allowed to drive the YM flash-swap callbacks. | Cannot be swapped out: `set_pool` is one-shot and set by the factory at deploy time to the pool the factory just created. |
| **Treasury owner** | Withdrawing collected protocol fees and upgrading/handing over the treasury contract. | Can drain or misdirect the *protocol's own* fees — never user funds: the treasury only ever receives the reserve cut of trading fees and post-maturity surplus, both of which belong to no user. It holds no lever over any live market (markets store its address, not the reverse). |
| **Router** | Nothing. It is an unprivileged convenience layer: no owner, no upgrade path, and no role in any other contract. Users authorize it directly, per call. | A deployed router cannot be subverted in place — it is immutable. A hostile *substitute* router address published to users could route them into markets of its choosing, but that is an off-chain distribution concern identical to a hostile frontend, and it is bounded: any router can only move funds the user separately authenticated in the same transaction (§3). |

The **vault boundary is the protocol's primary external dependency**. YBC does
not attempt to prove a vault is honest on-chain; it confines the blast radius
instead. Which markets are surfaced to end users is an off-chain concern.

---

## 2. Threat model

Assumptions that hold protocol-wide:

- **Soroban rejects re-entrancy.** A contract already on the call stack cannot be
  re-entered by a nested call. Several flows depend on this (see the rate-hint
  pattern in §4).
- **`require_auth` is the only authority.** A contract satisfies another
  contract's `require_auth` by being the *direct invoker* of the call; there is
  no signature to forge or replay.

### Actors and defenses

| Threat actor | Vector | Defense |
|--------------|--------|---------|
| Arbitrary caller of the AMM flash entrypoints | `flash_swap_pt` / `flash_swap_v` are not restricted to the router; anyone may call them. | The AMM only accepts the registered YM as `receiver`; the YM callback requires the registered pool as invoker; and every movement of the user's funds inside the callback is separately authenticated against `user`. A direct caller naming a victim as `user` cannot pull the victim's V/YT. (§4.5 of ARCHITECTURE) |
| Caller impersonating the pool to the YM | Call `on_flash_receive_*` directly. | `storage::get_pool().require_auth()` in both callbacks; only the real pool is the direct invoker during a genuine flash swap. |
| Caller pointing the router at a hostile market | Pass a `(vault, maturity)` naming an attacker-deployed YM or pool, so the router drives a user's funds into it. | Every router entrypoint resolves the market through `Factory::get_market` (`resolve_market`) and panics when there is no record, so only factory-deployed contracts are ever called. The factory keys markets by `(vault, maturity)` and refuses to overwrite an existing record. |
| Malicious / manipulated vault | Distort `convert_to_assets` to skew mint/redeem or AMM pricing. | Blast radius limited to that market. Only the YM reads the vault; it high-water-marks the rate, and the AMM prices against the YM's figure rather than the vault's, so a rate that *drops* cannot drag pool pricing or already-established payouts down. The rate also locks at maturity. |
| MEV / sandwich on swaps | Move the pool between quote and execution. | Every user-facing swap takes an explicit slippage bound (`v_in_max`, `min_v_out`, `max_v_in`, `min_shares_out`) and reverts if not met. Router zaps bound slippage in base-asset terms at the endpoint instead, measured from balance deltas — one number covering both the pool price and the vault rate (invariant 17). |
| Token donation to the pool | Send PT/V directly to the pool to distort pricing or share math. | Reserves are tracked in contract state and updated by *priced* amounts, not by reading balances; flash swaps assert exact balance deltas. Donated tokens never enter pricing. |
| First-depositor / share inflation | Seed a pool with dust to skew share accounting. | `MINIMUM_LIQUIDITY` (100 shares) is minted to a burn address on the first deposit, and the initial deposit must exceed it. |
| Fat-fingered market params | Absurd maturity or curve band. | Maturity must be in the future and within `MAX_MATURITY_HORIZON` (10y); the AMM constructor bounds the APY band and fee (§5). |

---

## 3. Privilege model

Who may call each privileged function, and whether it can be called more than once.

### Ownership (factory and treasury)

Both singletons use OpenZeppelin's `stellar-access` **`Ownable`** rather than a
hand-rolled admin. That gives each of them the same four entrypoints, with
identical semantics, on top of the `#[only_owner]` functions listed in their own
tables below. There is no `set_admin` / `transfer_admin` anywhere in the
codebase.

| Function | Authority | Notes |
|----------|-----------|-------|
| `get_owner` | None (view) | Returns `Option<Address>` — `None` after renouncement. |
| `transfer_ownership(new_owner, live_until_ledger)` | Current owner | **Step 1 of 2.** Records a pending owner; ownership does not move. The proposal **expires** at `live_until_ledger`, and passing `live_until_ledger = 0` cancels a pending transfer outright. Re-proposing overwrites. |
| `accept_ownership()` | **Pending owner** | Step 2 of 2. Control moves only when the proposee accepts, so a fat-fingered handoff cannot strand a live contract. Fails if there is no pending transfer, or it has expired. |
| `renounce_ownership()` | Current owner | **Irreversible and unrecoverable.** Deletes the owner entry, permanently disabling *every* `#[only_owner]` function on that contract. Refuses to run while a transfer is pending (`TransferInProgress`). See §6. |

### Factory (singleton, owner-controlled)

| Function | Authority | Notes |
|----------|-----------|-------|
| `create_market` | Permissionless (`creator.require_auth()`) | Anyone. Creator is recorded in `MarketCreated` only. |
| `set_wasm_hashes` | Owner | Affects only markets created afterward. |
| `set_fee_config` | Owner | Treasury address + reserve fee rate (bounded to 50% of the fee). **Prospective only**: snapshotted into each market at creation; live markets are untouched. |
| `upgrade` | Owner | Upgrades the factory contract Wasm. |
| `get_market`, `get_wasm_hashes`, `get_fee_config` | None | Views. |

Renouncing factory ownership permanently freezes `set_wasm_hashes`,
`set_fee_config`, and `upgrade`. `create_market` is permissionless and keeps
working, so the factory would go on deploying markets from whatever Wasm hashes
and fee config were current at that moment, forever.

### Yield Manager (per market; admin = the factory)

The factory is set as YM admin at deploy time, but its two setters are one-shot
and spent during `create_market`, so no ongoing admin control over a live market
remains.

| Function | Authority | Notes |
|----------|-----------|-------|
| `set_token_contracts` | Admin, **one-shot** (guarded by `is_initialized`) | Wires PT/YT. Second call returns `AlreadyInitialized`. |
| `set_pool` | Admin, **one-shot** (guarded by `is_pool_set`) | Registers the trusted flash-swap pool. Second call returns `PoolAlreadySet`. |
| `deposit`, `redeem_combined`, `redeem_principal` | The acting user (`from.require_auth()`) | Share-denominated. Value moves only for the authenticated user. |
| `deposit_asset`, `redeem_combined_to_asset`, `redeem_principal_to_asset` | The acting user (`from.require_auth()`) | Base-asset counterparts of the three above. The YM deposits into the vault with **itself** as receiver, or redeems from its **own** custody and has the vault pay the user directly, so no vault-share count ever reaches the user's signature (§4.8). |
| `exit_expired_to_asset` | The acting user (`from.require_auth()`), **plus caller-granted allowances** | A different authority shape from the rest of the table: it takes *ceilings*, not exact amounts, and consumes `min(ceiling, balance)` on each leg — `burn_from` against a PT allowance up to `max_pt`, `transfer_from` against a vault-share allowance up to `max_shares`. That is what lets freshly measured amounts (an LP payout, a yield claim) be redeemed without appearing in a signature. Unspent allowance is not consumed; see §6. |
| `distribute_yield` | The YT contract only (`yt_addr.require_auth()`) | Not user-callable; reached via `YT::claim_yield`. Once the rate is locked, re-denominates the payout to its locked-rate asset value. |
| `on_flash_receive_pt`, `on_flash_receive_v` | The registered pool only (`get_pool().require_auth()`) | Flash callbacks. |
| `collect_surplus` | **Permissionless** (no auth) | Sweeps accumulated post-maturity surplus to the treasury. Safe to open to anyone: the destination is the treasury address baked in at deploy, and the counter only ever holds shares no user has a claim on. |
| `get_exchange_rate` and the other getters | None | Views. `get_exchange_rate` is the figure the AMM prices against (§6). |

The treasury address is set by the factory at deploy time and has **no setter** —
like everything else about a live market, it is immutable.

### Principal Token (per market; admin = its YM)

| Function | Authority |
|----------|-----------|
| `mint` | YM only (`admin.require_auth()`) |
| `burn` / `burn_from` | Holder **and** YM (both `require_auth`) |
| `transfer` / `transfer_from` / `approve` | Standard token semantics (holder / spender auth) |

### Yield Token (per market; admin = its YM)

| Function | Authority | Notes |
|----------|-----------|-------|
| `mint` | YM only | Takes a rate hint (§4). |
| `burn_with_rate` / `transfer_with_rate` | Holder **and** YM | Rate-hinted variants used inside YM flows. |
| `burn` / `transfer` | Holder | Plain variants; fetch the rate from the YM directly. |
| `claim_yield` | The claiming user | Pulls accrued yield via YM `distribute_yield`; burns dust YT past maturity. Returns the shares actually paid (post-lock this can be fewer than the frozen accrual — same asset value). |
| `approve` / `transfer_from` / `burn_from` | **Unsupported** (panic) | YT deliberately omits allowance flows. |

### AMM (per market)

| Function | Authority |
|----------|-----------|
| `swap_v_for_pt`, `swap_pt_for_v`, `deposit`, `withdraw` | The acting user (`to.require_auth()`) |
| `flash_swap_pt`, `flash_swap_v` | Open to any caller, but `receiver` must equal the registered YM (asserted) |
| view functions | None |

The pool's fee config — treasury address and `reserve_fee_rate` — is written
once by the constructor (a snapshot of the factory's config at creation) and
**no setter exists**. Traders and LPs can read both at entry knowing they can
never be moved on a live market.

### Router (singleton, unprivileged)

The router holds **no privilege anywhere in the protocol**. It has no owner, no
`upgrade` entrypoint, and no setter for the factory address it was constructed
with — `storage.rs` exposes `set_factory`, called once from `__constructor` and
never again. Nothing about a deployed router can change.

It also **custodies nothing**: every leg names the user as the token-holding
party, so a revert anywhere strands no funds. The contracts it wraps stay public
and re-authenticate independently, so the router is a convenience, never a gate.

| Function | Authority | Notes |
|----------|-----------|-------|
| `swap_v_for_pt`, `swap_pt_for_v`, `swap_v_for_yt`, `swap_yt_for_v`, `deposit`, `withdraw`, `split`, `recombine`, `exit_expired` | The acting user (`to.require_auth()`) | Thin wrappers over the market's AMM/YM, after resolving the market through the factory. |
| `zap_*`, `exit_expired_to_asset` | The acting user (`to.require_auth()`) | Same, plus they grant token allowances on the user's behalf — see below. |
| `get_amm`, `get_reserves`, `balance_shares` | None | Views. |

Because a zap's real quantities are only knowable at execution time, the router
grants allowances *on the caller's behalf* mid-call rather than having the user
sign measured figures (invariant 16). Every one of these has caller-chosen
arguments, and every expiry is an **absolute ledger passed in by the caller** —
deriving it from the current ledger is precisely the drift this design exists to
avoid.

| Where | Allowance granted | Spender | How much is consumed |
|-------|-------------------|---------|----------------------|
| `split` | `shares_amount` vault shares, until `allow_expiry` | the market's YM | In full — nothing lingers. |
| `sweep_gained_shares` (every `zap_*` that settles in the base asset) | `sweep_allowance` vault shares, until `sweep_expiry` | **the router itself**, as SEP-56 operator | Only the measured `gained`; the remainder stays live until `sweep_expiry`. |
| `exit_expired_to_asset` | `max_pt` PT until `pt_allow_expiry`, and `sweep_allowance` vault shares until `sweep_expiry` | the market's YM | Only `min(ceiling, balance)` on each leg; the remainder stays live until its expiry. |

Residual allowance is the accepted cost of keeping measured amounts out of
signatures; §6 states the exposure and how to size the ceilings.

### Treasury (singleton, owner-controlled)

Passive fee sink: value arrives as plain token transfers from the per-market
contracts; the treasury does no fee accounting and holds no lever over any
market. It never custodies user funds — everything it holds is the protocol's.

| Function | Authority | Notes |
|----------|-----------|-------|
| `withdraw` | Owner | Any token, any amount. |
| `upgrade` | Owner | The address baked into markets is immutable, so in-place upgrade is the only way to evolve the treasury. Grants nothing `withdraw` doesn't already imply. |

Ownership moves through the two-step `Ownable` flow described above.
`renounce_ownership` is especially destructive here: with no owner, `withdraw`
is permanently bricked and **every fee the treasury holds or will ever receive
is stranded forever** — markets keep remitting to the address regardless, since
they store it immutably. This is covered by a regression test
(`renounce_bricks_withdraw`).

---

## 4. Core invariants

These are the properties the protocol is built to preserve. They are the natural
targets for review and for the property/fuzz tests (`proptests.rs`, the AMM fuzz
harness).

**Exchange rate**

1. The stored exchange rate is **monotonically non-decreasing** while the market
   is live: each update takes `max(stored, vault_rate)`.
2. At maturity the rate **locks** and never changes again.
3. PT redemption after maturity pays at `max(live_vault_rate, locked_rate)`, so
   the locked rate is a floor, never a cap, on PT payout.

**Token conservation**

4. `deposit` mints PT and YT in **equal** amounts; `redeem_combined` burns them in
   equal amounts. PT and YT supply move together outside of maturity redemption.
5. The YM **holds no PT across a call**: `on_flash_receive_pt` asserts its PT
   balance returns to zero before returning.
6. **Positions freeze in asset value at maturity.** PT redeems for exactly face
   value in assets, and a locked YT claim pays exactly its locked-rate asset
   value, no matter how late the exit — vault interest earned after maturity
   belongs to the protocol. Each exit above the locked rate frees the
   difference into the YM's surplus counter, which `collect_surplus` sweeps to
   the treasury. The counter only ever holds shares no user has a claim on, so
   PT redemption and YT claims remain open, and solvent, forever.

**Rounding**

7. All conversions **floor**, and always in the protocol's favor: mint amounts,
   share-return amounts, and accrued-yield payouts round down, leaving dust in the
   YM rather than overpaying a user.

**AMM**

8. Pricing uses **state-tracked reserves**, never raw balances, so donated tokens
   cannot influence a trade.
9. Reserves must remain **strictly positive** after any swap or flash swap.
10. Flash swaps assert **exact balance deltas**: the pool ends `flash_swap_pt` with
    exactly `yt_out` more PT and `v_paid` less V, and `flash_swap_v` fully repaid.
11. Curve parameters are bounded at construction: `apy_min < current_apy < apy_max`,
    `apy_max <= MAX_APY` (100%), band width `>= MIN_BAND_WIDTH` (1pp),
    `0 < fee_apy <= MAX_FEE_APY` (2%), and
    `0 <= reserve_fee_rate <= MAX_RESERVE_FEE_RATE` (50% of the fee).
12. **The treasury's fee cut never enters LP accounting.** The reserve cut of
    each trade's fee is remitted to the treasury inline and excluded from
    `reserve_b`, so stored reserves keep matching the pool's actual balances
    and LP withdrawals can never pay out protocol fees.

**Markets**

13. At most **one market per `(vault, maturity)`**; markets are immutable once
    created and never share state — including the treasury address and reserve
    fee rate, which have no setters.
14. `maturity` is strictly in the future and within `MAX_MATURITY_HORIZON` (10
    years). The upper bound mainly catches unit mistakes such as passing
    milliseconds, which would create a market that can never mature.

**Re-entrancy**

15. Inside the flash callbacks and combined-redeem, the YM passes the exchange
    rate to PT/YT as a **hint**, so those contracts never call back into the YM
    for it. That call would be re-entry while the YM is on the stack, which the
    host rejects; the hint keeps the flow single-pass.

**Router**

16. **Nothing the chain computes may enter a user's signature.** Soroban matches
    a signed authorization argument-for-argument at execution, and a wallet
    builds that signature by simulating beforehand — so a vault share count, a
    pool-priced amount, or a current ledger number is a guaranteed mismatch.
    Measured values move under *contract* authority instead: the AMM pulls the
    caller's bound and refunds, the YM redeems its own custody, and leftovers are
    swept against a caller-signed allowance whose amount and absolute expiry the
    caller chose. Violating this does not weaken a bound — it makes the call fail
    outright with `Auth/InvalidAction`, which is how the rule was found.
17. **Every router path that settles in the base asset is bounded in the base
    asset, at the end, from a measured balance delta.** This is why intermediate
    legs pass permissive per-leg bounds (`&1` to a swap, `&0` to an LP
    withdrawal): one terminal `min_asset_out` covers the pool price and the vault
    rate together, where per-leg bounds could not. **Removing a terminal assert
    silently removes all slippage protection from that path** — the permissive
    inner bounds are only sound because the terminal one exists.
18. **The router custodies nothing.** Every leg names the user as the
    token-holding party, so no balance is ever held by the router between legs
    and a revert anywhere strands no funds.
19. **Only factory-deployed contracts are ever called.** Markets are resolved by
    `(vault, maturity)` through the factory on every entrypoint, and a missing
    record panics.

---

## 5. Parameter bounds (reference)

| Constant | Value | Where | Purpose |
|----------|-------|-------|---------|
| `MAX_MATURITY_HORIZON` | 10 years (seconds) | factory | Upper bound on maturity; catches millisecond/fat-finger timestamps. |
| `MAX_APY` | `10_000_000` (100%) | AMM | Above this the fixed-point `exp`/`ln` approximations lose accuracy. |
| `MIN_BAND_WIDTH` | `100_000` (1pp) | AMM | Narrower bands make the curve so steep it rejects almost every trade. |
| `MAX_FEE_APY` | `200_000` (2%) | AMM | Above this trading is pointless. |
| `MAX_RESERVE_FEE_RATE` | `5_000_000` (50% **of the fee**, not of the trade) | factory + AMM | Above this the LP cut stops being worth providing liquidity for. Enforced at `set_fee_config` and again by the AMM constructor. |
| `MINIMUM_LIQUIDITY` | `100` shares | AMM | Burned on first deposit to prevent share-inflation on an empty pool. |

All APY values are 1e7-scaled (e.g. `500_000` = 5%).

---

## 6. Known limitations and accepted risks

- **Vault honesty is assumed, not verified.** The protocol relies on off-chain
  curation to decide which vaults/markets to surface. See §1.
- **Late YT claims pay locked value, not appreciated shares.** After the rate
  locks, a yield claim pays the shares its locked-rate asset value buys at the
  live rate — fewer shares than the frozen accrual when the vault has kept
  growing. This is the intended "no new yield after maturity" semantics, but it
  is a user-visible behavior worth surfacing in UIs: two claims of the same
  accrual made at different times receive different share counts (same asset
  value).
- **Renouncing ownership is irreversible and bricks the contract.**
  `renounce_ownership` is inherited from `Ownable` on both the factory and the
  treasury, and it is the most destructive privileged call in the system. On the
  treasury it permanently disables `withdraw`, stranding every fee held then or
  received later — and markets keep remitting to that address, because they
  store it immutably. On the factory it permanently freezes `set_wasm_hashes`,
  `set_fee_config`, and `upgrade` while leaving `create_market` permissionless,
  so the factory deploys from a frozen configuration forever. Neither is
  recoverable by any on-chain means. Treat it as a call that should never be
  made in production, not as an ownership option.
- **Treasury key management is off-chain.** The treasury owner can withdraw all
  collected fees and upgrade the treasury contract; compromise of that key
  loses protocol revenue (never user funds). The two-step ownership transfer
  mitigates fat-fingered handoffs, not key theft.
- **Router allowances can outlive the call that created them.** `split` sizes its
  allowance exactly and consumes it in full, but the sweep path and
  `exit_expired_to_asset` grant *ceilings* and spend only what the operation
  actually needs (§3). Whatever is left stays live until the caller-chosen expiry
  ledger. The exposure is bounded — the router is immutable and privilege-free,
  the YM's allowance-consuming entrypoints all require the owner's own auth in
  the same call, and both spenders can only move the value to its owner — but a
  caller who signs a large `sweep_allowance` or `max_pt` with a distant expiry is
  leaving a live approval behind. Size ceilings to the expected proceeds and keep
  expiries short.
- **No pause / emergency stop.** There is no admin switch to halt a live market;
  immutability is the deliberate trade-off. Recovery from a bad market is an
  off-chain curation decision, not an on-chain intervention.
- **The router address is a distribution concern.** The router is unprivileged
  and immutable, so it cannot be subverted in place — but users authorize it
  directly, and a hostile *substitute* address published to users routes them
  into markets of its choosing. This is the same trust surface as a frontend, and
  it is bounded by the fact that any router can only move funds the user
  separately authenticated in the same transaction.
- **Fixed-point math accuracy** near the parameter bounds is the subject of a
  dedicated math review; see the AMM curve/math modules and a future `MATH.md`.

---

## 7. Reporting

_Add a security contact / disclosure process here before any production
deployment (contact address, scope, and safe-harbor terms)._
