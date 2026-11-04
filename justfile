nightly := "nightly-2026-10-30"
msrv := "1.85"

# Every check, as CI runs it on Linux.
default: lint deps check compat

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

deny:
    cargo bin cargo-deny --config .config/deny.toml check

machete:
    cargo bin cargo-machete

# CI jobs: one recipe per job, built from the steps above.

lint: fmt-check spell clippy clippy-no-default
check: test doctest doc examples
deps: deny machete
compat: msrv no-default package

# CI job `deps`: every commit subject in the range is a Conventional Commit and names no version.
commits range="origin/main..HEAD":
    #!/usr/bin/env bash
    set -euo pipefail
    pattern='^(feat|fix|perf|refactor|style|test|docs|build|ci|chore|revert)(\([a-z0-9-]+\))?!?: [a-z]'
    version='(^|[^0-9.])v?0\.[0-9]+\.[0-9]+'
    bad=$(git log --no-merges --format='%h %s' "{{ range }}" | while read -r hash subject; do
        if ! [[ $subject =~ $pattern ]] || [[ $subject =~ $version ]]; then echo "  $hash $subject"; fi
    done)
    if [ -n "$bad" ]; then
        echo "commit subjects that are not 'type(scope): lower-case text' or that name a version:" >&2
        echo "$bad" >&2
        exit 1
    fi

# Clacky only: the rest of the CI jobs, then the local helpers.

# Build and test on the oldest supported Rust.
msrv:
    cargo +{{ msrv }} test --workspace --all-features --locked

# Build and test with no default features: no tokio in the tree.
no-default:
    cargo check --no-default-features --locked
    cargo bin cargo-nextest run --no-default-features --locked
    ! cargo tree --no-default-features -e normal | grep -q tokio

clippy-no-default:
    cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings

# The docs build without a warning, as docs.rs builds them.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked

# Both crates package and build from the packaged files.
package:
    cargo package --workspace --locked

# Every example builds with all features.
examples:
    cargo build --examples --all-features --locked

# The release size of a small form, built for size.
size:
    cargo build --profile size --example size --no-default-features
    wc -c < {{ env('CARGO_TARGET_DIR', 'target') }}/size/examples/size

# Keys per second of update and redraw for select, multiselect, autocomplete and text.
bench:
    cargo test --release --lib --features fake -- --ignored --nocapture keys_per_second

# Point git at the repository hooks (run once per clone).
hooks:
    git config core.hooksPath .githooks

# Bump versions and write changelogs for the crates that changed; review the diff, then commit `chore: release` and push.
release:
    cargo bin release-plz update
    git --no-pager diff

# Everything `default` runs, plus a dry-run publish of both crates.
release-check: default
    cargo publish --workspace --dry-run
