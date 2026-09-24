//! The asset-denominated paths: the yield manager and the router calling the
//! vault's `deposit` and `redeem` on the user's behalf. This is where the
//! delegated-auth shapes live (`from` != receiver, owner == operator == YM),
//! and where a WASM-level mismatch between adapter and core would surface.

use super::fixture::VaultStack;

/// `YM.deposit_asset`: the YM is the share receiver while the user is `from`
/// and `operator`. PT and YT mint against the shares the YM actually got.
fn ym_deposit_asset_mints_pt_and_yt(f: &VaultStack) {
    let user = f.user.clone();
    let amount = 1_000_0000000i128;
    f.mint_underlying(&user, amount);

    let minted = f.ym().deposit_asset(&user, &amount, &0);

    assert!(minted > 0);
    assert_eq!(f.pt_balance(&user), minted);
    assert_eq!(f.yt_balance(&user), minted);
    assert_eq!(f.underlying_balance(&user), 0, "the whole amount went in");
    assert!(f.vault_shares(&f.yield_manager) > 0, "YM custodies the shares");
    assert_eq!(f.vault_shares(&user), 0);
}

/// `YM.redeem_combined_to_asset`: the YM redeems from the vault as owner and
/// operator and pays the user in underlying.
fn ym_redeem_combined_to_asset_pays_the_underlying(f: &VaultStack) {
    let user = f.user.clone();
    let amount = 1_000_0000000i128;
    f.mint_underlying(&user, amount);
    let minted = f.ym().deposit_asset(&user, &amount, &0);

    let paid = f.ym().redeem_combined_to_asset(&user, &minted, &0);

    assert_eq!(f.underlying_balance(&user), paid);
    assert!(paid <= amount, "no free value: paid {paid} > deposited {amount}");
    assert!(paid >= amount - 2, "lost more than rounding: paid {paid} of {amount}");
    assert_eq!(f.pt_balance(&user), 0);
    assert_eq!(f.yt_balance(&user), 0);
    assert_eq!(f.vault_shares(&f.yield_manager), 0, "YM holds nothing after a full exit");
}

/// The router's zap: the production entry point for "underlying in, PT + YT
/// out", and its inverse.
fn router_zaps_underlying_to_split_and_back(f: &VaultStack) {
    let user = f.user.clone();
    let amount = 1_000_0000000i128;
    f.mint_underlying(&user, amount);

    let minted = f.router_zap_asset_for_split(&user, amount);
    assert!(minted > 0);
    assert_eq!(f.pt_balance(&user), minted);
    assert_eq!(f.yt_balance(&user), minted);
    assert_eq!(f.underlying_balance(&user), 0);

    let paid = f.router_zap_split_for_asset(&user, minted);
    assert_eq!(f.underlying_balance(&user), paid);
    assert!(paid <= amount && paid >= amount - 2, "round trip paid {paid} of {amount}");
    assert_eq!(f.pt_balance(&user), 0);
    assert_eq!(f.yt_balance(&user), 0);
}

/// Yield the YM has already accrued is paid to YT holders as vault shares,
/// which the holder can then redeem from the vault for underlying.
fn claimed_yield_is_redeemable_from_the_vault(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * super::fixture::ONE_DAY_SECS);
    f.accrue_interest();

    let claimed = f.claim_yield(&user);
    assert!(claimed > 0, "yield accrued");
    assert_eq!(f.vault_shares(&user), claimed);

    let paid = f.vault_client().redeem(&claimed, &user, &user, &user);
    assert!(paid > 0, "claimed shares are worth underlying");
    assert_eq!(f.underlying_balance(&user), paid);
}

on_every_vault!(
    ym_deposit_asset_mints_pt_and_yt,
    ym_redeem_combined_to_asset_pays_the_underlying,
    router_zaps_underlying_to_split_and_back,
    claimed_yield_is_redeemable_from_the_vault,
);
