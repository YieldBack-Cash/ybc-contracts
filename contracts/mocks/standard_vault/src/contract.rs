use soroban_sdk::{contract, contractimpl, Address, Env, MuxedAddress, String};
use stellar_tokens::fungible::{Base, FungibleToken};
use stellar_tokens::vault::Vault;

#[contract]
pub struct StandardVault;

#[contractimpl]
impl StandardVault {
    /// `decimals_offset` is the virtual-shares cushion OpenZeppelin uses to
    /// blunt inflation (donation) attacks on a nearly empty vault. Tests pass 0
    /// — the plain accounting — so share maths stay easy to reason about.
    pub fn __constructor(
        e: Env,
        asset: Address,
        decimals_offset: u32,
        name: String,
        symbol: String,
    ) {
        Vault::set_asset(&e, asset);
        Vault::set_decimals_offset(&e, decimals_offset);
        Base::set_metadata(&e, 7, name, symbol);
    }

    // ── SEP-56 (TokenizedVault) ─────────────────────────────────────────────
    //
    // Every body here delegates to OpenZeppelin's `Vault`, deliberately adding
    // no authorization of its own.
    //
    // SEP-56 leaves the auth pattern to implementers, so a test double is only
    // useful if it matches the vault actually shipped against. OZ's model —
    // `operator.require_auth()`, plus a consumed SEP-41 allowance from the
    // funds-owner when `operator` differs from `from`/`owner` — is exactly the
    // model blend-vault-v2 adopted when it closed the hole where `withdraw`
    // authenticated only the operator and let any caller drain any holder.
    //
    // So the two paths behave identically here and in production:
    //   * same address for every role → one signature, no allowance
    //   * operator acting for someone else → allowance required AND consumed
    //
    // An earlier revision added `owner.require_auth()` on the delegated path.
    // That was wrong twice over: it is stricter than either real vault, and it
    // would turn a missing-allowance failure into a missing-signature one,
    // hiding the error the router would actually hit.

    pub fn query_asset(e: &Env) -> Address {
        Vault::query_asset(e)
    }

    pub fn total_assets(e: &Env) -> i128 {
        Vault::total_assets(e)
    }

    pub fn convert_to_shares(e: &Env, assets: i128) -> i128 {
        Vault::convert_to_shares(e, assets)
    }

    pub fn convert_to_assets(e: &Env, shares: i128) -> i128 {
        Vault::convert_to_assets(e, shares)
    }

    pub fn max_deposit(e: &Env, receiver: Address) -> i128 {
        Vault::max_deposit(e, receiver)
    }

    pub fn max_redeem(e: &Env, owner: Address) -> i128 {
        Vault::max_redeem(e, owner)
    }

    pub fn preview_deposit(e: &Env, assets: i128) -> i128 {
        Vault::preview_deposit(e, assets)
    }

    pub fn preview_redeem(e: &Env, shares: i128) -> i128 {
        Vault::preview_redeem(e, shares)
    }

    pub fn deposit(
        e: &Env,
        assets: i128,
        receiver: Address,
        from: Address,
        operator: Address,
    ) -> i128 {
        Vault::deposit(e, assets, receiver, from, operator)
    }

    pub fn redeem(
        e: &Env,
        shares: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        Vault::redeem(e, shares, receiver, owner, operator)
    }
}

#[contractimpl]
impl FungibleToken for StandardVault {
    /// `Vault` rather than `Base`: it overrides `decimals` to account for the
    /// virtual offset, and share accounting must go through the same type the
    /// SEP-56 entrypoints above use.
    type ContractType = Vault;

    fn total_supply(e: &Env) -> i128 {
        Base::total_supply(e)
    }

    fn balance(e: &Env, account: Address) -> i128 {
        Base::balance(e, &account)
    }

    fn allowance(e: &Env, owner: Address, spender: Address) -> i128 {
        Base::allowance(e, &owner, &spender)
    }

    fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
        Base::transfer(e, &from, &to, amount)
    }

    fn transfer_from(e: &Env, spender: Address, from: Address, to: Address, amount: i128) {
        Base::transfer_from(e, &spender, &from, &to, amount)
    }

    fn approve(e: &Env, owner: Address, spender: Address, amount: i128, live_until_ledger: u32) {
        Base::approve(e, &owner, &spender, amount, live_until_ledger)
    }

    fn decimals(e: &Env) -> u32 {
        Vault::decimals(e)
    }

    fn name(e: &Env) -> String {
        Base::name(e)
    }

    fn symbol(e: &Env) -> String {
        Base::symbol(e)
    }
}
