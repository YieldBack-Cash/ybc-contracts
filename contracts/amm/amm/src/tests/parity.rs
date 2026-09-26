//! Pins `math::implied_rate_to_exchange_rate` to the JSON fixture the apps'
//! parity test checks `protocol/protocol.ts`'s floating-point PT price against. Run with
//! `YBC_WRITE_FIXTURES=1` to regenerate after changing the curve.

extern crate std;
use crate::math::implied_rate_to_exchange_rate;
use std::{format, fs, path::PathBuf, string::String};

// 1e7-scaled ln-space rates: 0, 0.01%, 1%, 5%, 10%, 20%, 50%, 100% a year.
const RATES: [i128; 8] = [0, 1_000, 100_000, 500_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000];
// One second, a day, a month, half a year, a year, two years.
const SECS: [i128; 6] = [1, 86_400, 2_592_000, 15_768_000, 31_536_000, 63_072_000];

fn render() -> String {
    let mut rows = std::vec::Vec::new();
    for &rate in &RATES {
        for &secs in &SECS {
            let exchange_rate = implied_rate_to_exchange_rate(rate, secs).unwrap();
            rows.push(format!(
                "  {{\"impliedRate\": \"{rate}\", \"secondsToExpiry\": \"{secs}\", \"exchangeRate\": \"{exchange_rate}\"}}"
            ));
        }
    }
    format!("[\n{}\n]\n", rows.join(",\n"))
}

#[test]
fn typescript_fixture_matches() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../protocol/fixtures/pt_exchange_rate.json");
    let fresh = render();
    if std::env::var_os("YBC_WRITE_FIXTURES").is_some() {
        fs::write(&path, &fresh).expect("write fixture");
        return;
    }
    match fs::read_to_string(&path) {
        Ok(on_disk) => assert_eq!(
            on_disk, fresh,
            "pt_exchange_rate.json is stale; rerun with YBC_WRITE_FIXTURES=1"
        ),
        Err(_) => std::eprintln!("skipped: {} not checked out", path.display()),
    }
}
