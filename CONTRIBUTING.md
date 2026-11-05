# Contributing

## Setup

```sh
cargo install cargo-run-bin --locked
rustup toolchain install
rustup toolchain install "$(cargo bin just --evaluate nightly)" --profile minimal --component rustfmt
cargo bin just hooks
```

- `rust-toolchain.toml` pins the stable toolchain, and `rustup toolchain install` installs it. The nightly named in `justfile` is used only for formatting.
- Every other tool, `just` included, is pinned in `Cargo.toml` and runs as `cargo bin <tool>`. It builds into `.bin/` on first use, or comes prebuilt when [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) is installed.
- `cargo bin just hooks` points git at `.githooks/`: `pre-commit` runs `fmt-check` and `spell` (seconds), `commit-msg` checks the subject, `pre-push` runs `clippy`.

## Checks

- `cargo bin just` runs `default`, which runs every group. Run it before a pull request.
- Each recipe runs one tool, and a group only lists recipes. `cargo bin just --list` shows both; `cargo bin just clippy` or `cargo bin just lint` runs one.
- `cargo bin just fmt` formats the code.
- Recipes call each tool as `cargo bin cargo-<tool>`, not through a cargo alias, which would shadow a copy installed globally.
- CI (`.github/workflows/ci.yml`) runs the same recipes after the shared setup action `.github/actions/setup`. Each runner in its matrix names its recipes. On Linux CI also checks every commit subject in the push or pull request with `cargo bin just commits <range>`.
- The spell check is [`typos`](https://github.com/crate-ci/typos). A real word it flags goes under `[default.extend-words]` in `_typos.toml`.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`, the subject in lower case and without a version number. The `commit-msg` hook refuses anything else, and `cargo bin just commits` checks a range (`origin/main..HEAD` by default). Scopes are listed in the project section below.

The type does not decide the version or the changelog: the maintainer picks the level at release and the changelog is written by hand. A breaking change takes `!` after the type (`feat!: ...`).

## Releases

- Each crate has its own version, a `CHANGELOG.md` in its directory and a tag `<crate>-v<version>`.
- Write the changelog by hand under `## Unreleased`, in the same commit as the change: [Keep a Changelog](https://keepachangelog.com) groups `Added`, `Changed`, `Fixed` and `Removed`, one line per change that starts with what the user types or sees, no scopes. A breaking change goes in a first paragraph of the version, `Breaking: ...`, that says what to do.
- `cargo bin just ship <level>` (`patch`, `minor` or `major`) runs every check, the semver check and a dry run of `cargo-release`. It needs a clean `main`. Add `execute` (`cargo bin just ship patch execute`) to release for real: it bumps the versions, renames `## Unreleased` to `## <version> (<date>)`, commits `chore: release`, tags, publishes and pushes. Run `cargo login` once on the maintainer's machine first.
- A tag push runs the `release` workflow, which makes a draft GitHub release from the changelog section with `cargo bin just release-notes <tag>` and then publishes it. It fails when the section is missing or empty, and it runs only in the `easymikey` repository. CI holds no crates.io token.

## Layout

```
src/
  core/        the `Ask` trait, `Session`, settings, keys and errors
  prompts/     one module per prompt: the builder and its keys-to-state machine
  output/      log entries, boxes, streamed text, spinner, progress bar, tasks
  render/      rails, frames, wrapping, colour, styles and symbols
  terminal/    the `Terminal` trait, `Console` over the real terminal, key reading
  values/      dates, paths and text lines the prompts edit
  fake.rs      scripted terminal and screen emulator for tests (feature `fake`)
  prelude.rs   `use clacky::prelude::*;`
clacky-derive/ `#[derive(Choices)]` and `#[derive(Form)]` (feature `derive`)
examples/      runnable examples, one per topic
docs/          guides: prompts, forms, live output, abort, theming, testing, size
tests/         derive tests (features `derive` and `fake`)
```

## Clacky

- Scopes: `derive` for `clacky-derive`, a module name for the rest.
- The MSRV is 1.85 (`msrv` in `justfile`). Install it once with `rustup toolchain install "$(cargo bin just --evaluate msrv)" --profile minimal`.
- Groups: `lint` (`fmt-check`, `spell`, `clippy`), `check` (`test`, `doctest`, `doc`), `deps` (`deny`, `machete`), `compat` (`clippy-no-default`, `msrv`, `no-default`, `package`, `examples`).
- `no-default` builds and tests without default features and fails if `tokio` is in the normal dependency tree with all features.
- CI runs `default` on Linux, and `clippy clippy-no-default check examples` on macOS and Windows.
- `size` (see `docs/size.md`) and `bench` are local helpers outside `default`.
- Tags are `clacky-vX.Y.Z` and `clacky-derive-vX.Y.Z`. `clacky` pins `clacky-derive` with `=`, so a derive release bumps both crates.
- `cargo bin just ship` runs `cargo-semver-checks` (pinned like the other tools) before the release step; an API break it finds needs a higher level.
