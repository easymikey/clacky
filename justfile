nightly := "nightly-2026-10-30"
msrv := "1.85"

default: lint deps check compat

lint: fmt-check spell clippy
check: test doctest doc
deps: deny machete
compat: clippy-no-default msrv no-default package examples

fmt:
    cargo +{{ nightly }} fmt --all

fmt-check:
    cargo +{{ nightly }} fmt --all --check

spell:
    cargo bin cargo-spellcheck check -m 1

clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
    cargo bin cargo-nextest run --workspace --all-features --locked

doctest:
    cargo test --workspace --all-features --locked --doc

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked

deny:
    cargo bin cargo-deny --config .config/deny.toml check

machete:
    cargo bin cargo-machete

commits range="origin/main..HEAD":
    #!/usr/bin/env bash
    set -euo pipefail
    git log --no-merges --format=%s "{{ range }}" | {
        status=0
        while IFS= read -r subject; do
            printf '%s\n' "$subject" | .githooks/commit-msg /dev/stdin || status=1
        done
        exit "$status"
    }

hooks:
    git config core.hooksPath .githooks

release:
    cargo bin cargo-semver-checks --version
    PATH="$(printf '%s:' "$PWD"/.bin/rust-*/cargo-semver-checks/*/bin)$PATH" cargo bin release-plz update

publish:
    cargo bin release-plz release --git-token "${GITHUB_TOKEN:?set GITHUB_TOKEN}"

clippy-no-default:
    cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings

msrv:
    cargo +{{ msrv }} test --workspace --all-features --locked

no-default:
    cargo check --no-default-features --locked
    cargo bin cargo-nextest run --no-default-features --locked
    ! cargo tree --no-default-features -e normal | grep -q tokio

package:
    cargo package --workspace --locked

examples:
    cargo build --examples --all-features --locked

size:
    cargo build --profile size --example size --no-default-features
    wc -c < {{ env('CARGO_TARGET_DIR', 'target') }}/size/examples/size

bench:
    cargo test --release --lib --features fake -- --ignored --nocapture keys_per_second
