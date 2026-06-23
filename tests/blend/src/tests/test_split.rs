use soroban_sdk::Env;

use super::fixture::BlendFixture;

/// Deposit underlying tokens into the fee vault, then split the resulting shares
/// into PT + YT via the yield manager. Verifies that:
///   - fee vault deposit returns a non-zero share amount
///   - after ym_deposit the shares move from the user to the yield manager
///   - PT and YT are minted in equal amounts to the user
#[test]
fn split_fee_vault_shares_into_pt_yt() {
    let env = Env::default();
    let f = BlendFixture::new(&env);

    let user = f.user.clone();
    let deposit_amount = 1_000_0000000i128; // 1 000 underlying tokens (7 decimals)

    // ── Step 1: seed user with underlying asset ───────────────────────────
    f.mint_asset(&user, deposit_amount);

    // ── Step 2: deposit underlying tokens into fee vault, receive shares ──
    // deposit(assets, receiver, from, operator) → shares_minted
    let shares = f.fee_vault.deposit(&deposit_amount, &user, &user, &user);
    assert!(shares > 0, "fee vault deposit should return shares");
    assert_eq!(
        f.fee_vault.get_shares(&user),
        shares,
        "get_shares should match minted amount"
    );

    // ── Step 3: split fee vault shares into PT + YT ───────────────────────
    f.ym_deposit(&user, shares);

    // ── Step 4: assertions ────────────────────────────────────────────────
    let pt = f.pt_balance(&user);
    let yt = f.yt_balance(&user);

    assert!(pt > 0, "PT should be minted after split");
    assert_eq!(pt, yt, "PT and YT must be minted 1:1");

    // Shares moved from user to yield manager
    assert_eq!(
        f.fee_vault.get_shares(&user),
        0,
        "user should hold no fee vault shares after split"
    );
    assert_eq!(
        f.fee_vault.balance(&f.yield_manager),
        shares,
        "yield manager should custody the deposited shares"
    );
}