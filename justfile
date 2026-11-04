nightly := "nightly-2026-10-30"
msrv := "1.85"

# Every check, as CI runs it.
default: fmt-check clippy clippy-no-default test doctest doc examples msrv no-default package

fmt:
    cargo +{{ nightly }} fmt --all

fmt-check:
    cargo +{{ nightly }} fmt --all --check

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo bin cargo-nextest run --workspace --all-features

doctest:
    cargo test --workspace --doc --all-features

# Build and test on the oldest supported Rust.
msrv:
    cargo +{{ msrv }} test --workspace --all-features

# Build and test with no default features: no tokio in the tree.
no-default:
    cargo check --no-default-features
    cargo bin cargo-nextest run --no-default-features
    ! cargo tree --no-default-features -e normal | grep -q tokio

clippy-no-default:
    cargo clippy --workspace --all-targets --no-default-features -- -D warnings

# The docs build without a warning, as docs.rs builds them.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

# Both crates package and build from the packaged files.
package:
    cargo package --workspace

# Every example builds with all features.
examples:
    cargo build --examples --all-features

ci: fmt-check clippy clippy-no-default test doctest doc examples msrv no-default package

# Keys per second of update and redraw for select, multiselect, autocomplete and text.
bench:
    cargo test --release --lib --features fake -- --ignored --nocapture keys_per_second

# The release size of a small form, built for size.
size:
    cargo build --profile size --example size --no-default-features
    wc -c < {{ env('CARGO_TARGET_DIR', 'target') }}/size/examples/size
