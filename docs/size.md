# Size and comparison

`just size` builds `examples/size.rs` (text, select and confirm) with `opt-level = "z"`, LTO and stripped symbols, without default features: about 410 KiB with Rust 1.97.1 on macOS. clacky has 4 dependencies (`libc`, `unicode-segmentation`, `unicode-width`, `zeroize`), async API included.

Measured on 2026-11-04 with Rust 1.97.1 on macOS. "Transitive" counts the distinct crates in `cargo tree -e normal` for the default features. "Binary" is a release build of a one-question example with `opt-level = "s"`, LTO, stripped symbols, `codegen-units = 1` and `panic = "abort"`.

| | clacky | cliclack 0.5.7 | dialoguer 0.12.0 | inquire 0.9.4 | demand 2.4.1 |
|---|---|---|---|---|---|
| Transitive crates | 4 | 41 | 13 | 30 | 12 |
| Binary, one question | 377 KiB | 409 KiB | 312 KiB | 444 KiB | 377 KiB |
| Theme | value on the session | global `RwLock` | value, per prompt | global and per prompt | value, per prompt |
| Test your own form | yes, `Script` and `simulate` | no | no | no | no |
| Select gives your value | yes | yes | no, an index | yes | yes |
| Piped input | per prompt: line or `NoInput` | error | error | `NotTTY` error | text reads a line, select does not |
| Prefill and `--yes` | `.given`, `Defaults` | no | no | no | no |
| MSRV | 1.85 | not set | 1.66 | 1.80 | not set (edition 2024) |

dialoguer and inquire have years of use and fuzzy-select or editor prompts, and inquire has more prompt types. clacky is 0.1 and has no back navigation, no form with several fields on one screen, and no editor prompt.
