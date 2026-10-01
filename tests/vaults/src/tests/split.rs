//! Market creation and the share-denominated path: vault shares in, PT + YT
//! out, and back.

use super::fixture::VaultStack;

/// The factory's YM constructor reads `convert_to_assets` from a vault that has
/// never taken a deposit. The old blend vault divided by zero there and needed
/// a bootstrap deposit; neither adapter does now.
fn market_is_created_on_an_empty_vault(f: &VaultStack) {
    assert_eq!(f.vault_client().total_supply(), 0);
    assert!(
        f.ym().get_exchange_rate() > 0,
        "rate must be readable on an empty vault"
    );
    assert_eq!(f.ym().get_vault(), f.vault);
    assert_eq!(f.vault_client().query_asset(), f.underlying);
}

/// Deposit underlying into the vault, then split the shares into PT + YT via
/// the yield manager.
fn split_vault_shares_into_pt_yt(f: &VaultStack) {
    let user = f.user.clone();
    let deposit = 1_000_0000000i128;
    f.mint_underlying(&user, deposit);

    let shares = f.vault_deposit(&user, deposit);
    assert!(shares > 0, "vault deposit should return shares");
    assert_eq!(f.vault_shares(&user), shares);

    f.vault_approve(&user, &f.yield_manager, shares);
    f.ym().deposit(&user, &shares);

    let pt = f.pt_balance(&user);
    let yt = f.yt_balance(&user);
    assert!(pt > 0, "PT should be minted after split");
    assert_eq!(pt, yt, "PT and YT must be minted 1:1");

    assert_eq!(
        f.vault_shares(&user),
        0,
        "user holds no vault shares after split"
    );
    assert_eq!(
        f.vault_shares(&f.yield_manager),
        shares,
        "yield manager custodies the deposited shares"
    );
}

/// Recombining PT + YT returns the shares, and redeeming those from the vault
/// returns the underlying, less at most a unit of rounding per hop.
fn shares_round_trip_through_the_yield_manager(f: &VaultStack) {
    let user = f.user.clone();
    let deposit = 1_000_0000000i128;
    f.setup_yt_position(&user, deposit);
    let minted = f.pt_balance(&user);

    f.ym().redeem_combined(&user, &minted);
    assert_eq!(f.pt_balance(&user), 0);
    assert_eq!(f.yt_balance(&user), 0);
    let shares = f.vault_shares(&user);
    assert!(shares > 0, "shares came back from the yield manager");

    let paid = f.vault_client().redeem(&shares, &user, &user, &user);
    assert_eq!(f.underlying_balance(&user), paid);
    assert!(
        paid <= deposit,
        "no free value: paid {paid} > deposited {deposit}"
    );
    assert!(
        paid >= deposit - 2,
        "lost more than rounding: paid {paid} of {deposit}"
    );
    assert_eq!(f.vault_shares(&user), 0);
}

on_every_vault!(
    market_is_created_on_an_empty_vault,
    split_vault_shares_into_pt_yt,
    shares_round_trip_through_the_yield_manager,
);
