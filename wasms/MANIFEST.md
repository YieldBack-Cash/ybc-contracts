# Vendored YBC vault adapters

Test fixtures only; nothing here ships. `tests/vaults` deploys both adapters
beside the real factory so that every property of "a YBC market" is asserted
against each vault rather than against one and assumed for the other.

Source: the `ybc-vaults` release `v0.1.2` (commit `305fc05`), downloaded
unchanged from the GitHub releases `v0.1.2_blend-vault_cli27.0.0` and
`v0.1.2_xoxno-vault_cli27.0.0`. Built there by the release workflow:
`stellar contract build --optimize` with the `source_repo` and `home_domain`
metadata, Stellar CLI 27.0.0, Rust 1.99.0. `SHA256SUMS` beside this file is
the checksum manifest; CI verifies it with `sha256sum -c SHA256SUMS`.

| File | Hash |
|---|---|
| `blend_vault.wasm` | `fc5ee50c…` |
| `xoxno_vault.wasm` | `4a4c6cad…` |

To refresh after a `ybc-vaults` change: take the binaries from its new
release (not a local build; a Windows build can differ in layout), copy them
here under these names, rewrite `SHA256SUMS`, and record the release here.
