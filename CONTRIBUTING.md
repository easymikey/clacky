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
- `cargo bin just hooks` points git at `.githooks/`: `pre-commit` runs `fmt-check` and `clippy`, `commit-msg` checks the subject.
- The spell check builds `cargo-spellcheck` against libclang. On Linux install `libclang-dev`. On macOS the Xcode command line tools have it; build it once with `RUSTFLAGS="-C link-arg=-Wl,-rpath,/Library/Developer/CommandLineTools/usr/lib" cargo bin cargo-spellcheck --version`.

## Checks

- `cargo bin just` runs `default`, which runs every group. Run it before a pull request.
- Each recipe runs one tool, and a group only lists recipes. `cargo bin just --list` shows both; `cargo bin just clippy` or `cargo bin just lint` runs one.
- `cargo bin just fmt` formats the code.
- Recipes call each tool as `cargo bin cargo-<tool>`, not through a cargo alias, which would shadow a copy installed globally.
- CI (`.github/workflows/ci.yml`) runs the same recipes after the shared setup action `.github/actions/setup`. Each runner in its matrix names its recipes. On Linux CI also checks every commit subject in the push or pull request with `cargo bin just commits <range>`.
- A real word the spell check flags goes into `spellcheck.dic`, one per line, and the count on its first line goes up by one.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`, the subject in lower case and without a version number. The `commit-msg` hook refuses anything else, and `cargo bin just commits` checks a range (`origin/main..HEAD` by default). Scopes are listed in the project section below.

- `feat` goes into the changelog under "Added".
- `fix` goes under "Fixed".
- `refactor`, `perf` and `revert` go under "Changed".
- `docs`, `test`, `build`, `ci`, `chore` and `style` stay out of the changelog.
- A breaking change takes `!` after the type (`feat!: ...`) or a `BREAKING CHANGE:` footer, and always reaches the changelog.
- While a crate is at 0.x, `feat` and `fix` bump the patch version and a breaking change bumps the minor version.

## Releases

- Each crate has its own version, a `CHANGELOG.md` in its directory, a tag `<crate>-v<version>` and a GitHub release. [release-plz](https://release-plz.dev) (`release-plz.toml`) picks each changed crate's bump from the commits.
- A push to `main` runs CI and publishes nothing, unless its last commit is `chore: release` and CI passes.
- To release from GitHub, run the `release` workflow on `main` from the Actions tab. It runs `cargo bin just release`, commits `chore: release`, pushes it and runs `cargo bin just publish`.
- To release locally, run `cargo bin just release`, review the diff, commit it as `chore: release` and push. Once CI on `main` passes, the `release` workflow runs `cargo bin just publish`.
- `cargo bin just publish` needs `GITHUB_TOKEN` and `CARGO_REGISTRY_TOKEN`. It publishes each crate version crates.io lacks, tags it and creates its GitHub release; with nothing new it does nothing.
- The `release` workflow runs only in the `easymikey` repository, so a fork never publishes.

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
- `no-default` builds and tests without default features and fails if `tokio` is in the normal dependency tree.
- CI runs `default` on Linux, and `clippy clippy-no-default check examples` on macOS and Windows.
- `size` (see `docs/size.md`) and `bench` are local helpers outside `default`.
- Tags are `clacky-vX.Y.Z` and `clacky-derive-vX.Y.Z`. `clacky` pins `clacky-derive` with `=`, so a derive release bumps both crates.
- `cargo bin just release` also runs `cargo-semver-checks` (pinned like the other tools), and an API break it finds sets the bump.
