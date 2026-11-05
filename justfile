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
    cargo bin typos

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

ci-setup:
    rustup toolchain install {{ nightly }} --profile minimal --component rustfmt
    rustup toolchain install {{ msrv }} --profile minimal

hooks:
    git config core.hooksPath .githooks

# Checks, the semver check, then a dry run of the release. Pass `execute` to do it.
# Below 1.0 cargo-semver-checks calls an API break "major", while the 0.x bump for it is minor.
ship level execute="":
    cargo bin just default
    cargo bin cargo-semver-checks check-release --workspace --release-type {{ if level == "patch" { "patch" } else { "major" } }}
    cargo bin cargo-release {{ level }} --workspace {{ if execute == "execute" { "--execute" } else { "--no-verify" } }}
# Print the changelog section of a tag such as clacky-v0.2.0.
release-notes tag:
    #!/usr/bin/env bash
    set -euo pipefail
    tag="{{ tag }}"
    crate="${tag%-v*}"
    version="${tag##*-v}"
    if [ "$crate" = "$tag" ] || [ -z "$crate" ] || [ -z "$version" ]; then
        echo "release-notes: '$tag' is not <crate>-v<version>" >&2
        exit 1
    fi
    dir="$(cargo pkgid -p "$crate")"
    dir="${dir#path+file://}"
    dir="${dir%%#*}"
    file="$dir/CHANGELOG.md"
    notes="$(awk -v head="## $version (" '
        /^## / { if (found) exit; found = index($0, head) == 1; next }
        found { print }
    ' "$file")"
    if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
        echo "release-notes: no notes for $version in $file" >&2
        exit 1
    fi
    printf '%s\n' "$notes" | awk 'NF { started = 1 } started'
clippy-no-default:
    cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings

msrv:
    cargo +{{ msrv }} test --workspace --all-features --locked

no-default:
    cargo check --no-default-features --locked
    cargo bin cargo-nextest run --no-default-features --locked
    ! cargo tree --all-features -e normal | grep -q tokio

package:
    cargo package --workspace --locked

examples:
    cargo build --examples --all-features --locked

size:
    cargo build --profile size --example size --no-default-features
    wc -c < {{ env('CARGO_TARGET_DIR', 'target') }}/size/examples/size

bench:
    cargo test --release --lib --features fake -- --ignored --nocapture keys_per_second
