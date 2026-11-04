# Size and comparison

`just size` builds `examples/size.rs` (text, select and confirm) with
`opt-level = "z"`, LTO and stripped symbols, without default features:
419,328 bytes with Rust 1.97.1 on macOS, about 410 KiB. It moves a little
with every compiler and commit.

Dependencies are the distinct crates below `clacky` in
`cargo tree -e normal` on macOS. Without `tokio` there are 4: `libc`,
`unicode-segmentation`, `unicode-width` and `zeroize` (on Windows
`windows-sys` and `windows-link` instead of `libc`). With the default
`tokio` there are 11: `tokio`, `tokio-macros`, `pin-project-lite`,
`proc-macro2`, `quote`, `syn` and `unicode-ident` join the four.

## Compared

Measured on 2026-11-04 with Rust 1.97.1 on macOS. "Transitive" is the distinct
crates (name and version) in `cargo tree -e normal` for the default features,
not counting the crate itself or the example. "Binary" is a release build of a
one-question example with `opt-level = "s"`, LTO, stripped symbols,
`codegen-units = 1` and `panic = "abort"`, the same for every column.

| | clacky | cliclack 0.5.7 | dialoguer 0.12.0 | inquire 0.9.4 | demand 2.4.1 |
|---|---|---|---|---|---|
| Transitive crates | 4 (11 with `tokio`) | 41 | 13 | 30 | 12 |
| Binary, one question | 377 KiB | 409 KiB | 312 KiB | 444 KiB | 377 KiB |
| Theme | value on the session | global `RwLock` | value, per prompt | global and per prompt | value, per prompt |
| Test your own form | yes, `Script` and `simulate` | no | no | no | no |
| Select gives your value | yes | yes | no, an index | yes | yes |
| Piped input | per prompt: line or `NoInput` | error | error | `NotTTY` error | text reads a line, select does not |
| Prefill and `--yes` | `.given`, `Defaults` | no | no | no | no |
| MSRV | 1.85 | not set | 1.66 | 1.80 | not set (edition 2024) |

Where the others are ahead: dialoguer and inquire have years of use and
fuzzy-select or editor prompts, and inquire has more built-in prompt types.
clacky is 0.1 and has no back navigation, no form with several fields on
one screen, and no editor prompt.
