//! The accrual formula, on its own so it can be pinned: the TypeScript side
//! (`protocol/protocol.ts`) reproduces it to show pending yield before a claim, and
//! `fixtures/yt_pending_yield.json` is what keeps the two in step.

use ybc_common::scale::SCALAR_7;

/// Vault shares owed to a holder of `balance` YT (asset-denominated) since
/// they last settled at `old_index`, now that the rate is `current_rate`.
///
/// `balance * (current_rate - old_index) / old_index` is the accrued yield in
/// asset units; it is paid out in shares at the current price, hence the
/// division by `current_rate` (rescaled by `SCALAR_7`). Floors, so the payout
/// rounds down and the yield manager keeps a dust surplus. Zero when nothing
/// has accrued or the inputs are degenerate.
pub fn pending_yield(balance: i128, old_index: i128, current_rate: i128) -> i128 {
    if balance <= 0 || old_index <= 0 || current_rate <= old_index {
        return 0;
    }
    balance
        .checked_mul(current_rate - old_index)
        .and_then(|v| v.checked_mul(SCALAR_7))
        .expect("overflow computing pending yield")
        / old_index
            .checked_mul(current_rate)
            .expect("overflow computing yield denominator")
}

/// Pins `pending_yield` to the JSON fixture the apps' parity test checks
/// `protocol/protocol.ts` against.
/// Run with `YBC_WRITE_FIXTURES=1` to regenerate the fixture after changing
/// the formula; the TypeScript parity test then fails until it is updated too.
#[cfg(test)]
mod parity {
    extern crate std;
    use super::pending_yield;
    use std::{format, fs, path::PathBuf, string::String};

    const BALANCES: [i128; 5] = [1, 12_345, 1_0000000, 1_000_000_0000000, 500_000_000_0000000];
    const INDEXES: [i128; 3] = [1_0000000, 1_5000000, 2_0000000];
    const RATE_STEPS: [i128; 5] = [0, 1, 12_345, 1_000000, 1_0000000];

    fn render() -> String {
        let mut rows = std::vec::Vec::new();
        for &balance in &BALANCES {
            for &index in &INDEXES {
                for &step in &RATE_STEPS {
                    let rate = index + step;
                    rows.push(format!(
                        "  {{\"balance\": \"{balance}\", \"userIndex\": \"{index}\", \"exchangeRate\": \"{rate}\", \"pending\": \"{}\"}}",
                        pending_yield(balance, index, rate)
                    ));
                }
            }
        }
        format!(
            "[
{}
]
",
            rows.join(
                ",
"
            )
        )
    }

    /// Skips (passes) when the `protocol/` tree is not checked out.
    #[test]
    fn typescript_fixture_matches() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../protocol/fixtures/yt_pending_yield.json");
        let fresh = render();
        if std::env::var_os("YBC_WRITE_FIXTURES").is_some() {
            fs::write(&path, &fresh).expect("write fixture");
            return;
        }
        match fs::read_to_string(&path) {
            Ok(on_disk) => assert_eq!(
                on_disk, fresh,
                "yt_pending_yield.json is stale; rerun with YBC_WRITE_FIXTURES=1"
            ),
            Err(_) => std::eprintln!("skipped: {} not checked out", path.display()),
        }
    }
}
