# Contributing

## Set up

Install [`cargo-run-bin`](https://github.com/dustinblackman/cargo-run-bin)
(`cargo install cargo-run-bin --locked`, or `cargo binstall cargo-run-bin`).
Every other tool is pinned in `Cargo.toml` under `[package.metadata.bin]` and
runs as `cargo bin <tool>`, including `just`. With `cargo-binstall` installed
the tools come as prebuilt binaries instead of builds from source.

Run `cargo bin just hooks` once per clone. A commit then runs the format check
and clippy, and a commit message that is not a Conventional Commit is refused.

## Before a pull request

Run `cargo bin just`. It runs every check CI runs on Linux. Each check is its
own recipe (`cargo bin just --list`), and groups only list recipes: `lint`,
`check`, `deps`, `compat`. Format code with `cargo bin just fmt`.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`,
with a lower-case subject that names no version. `cargo bin just commits`
checks a range of commits.

- `feat`: a new feature. Becomes "Added" in the changelog and bumps the minor version.
- `fix`: a bug fix. Becomes "Fixed" and bumps the patch version.
- `refactor`, `perf`, `revert`: become "Changed".
- `docs`, `test`, `build`, `ci`, `chore`, `style`: kept out of the changelog.

Mark a breaking change with `!` (`feat!: ...`) or a `BREAKING CHANGE:` footer.

## Releases

Commits pile up on `main`, and every push there runs the checks. A release
happens when the maintainer decides, in one of two ways with the same result:

- Run the `release` workflow from the Actions tab. It runs `just release`,
  commits `chore: release` to `main`, then runs `just publish`.
- Run `just release` locally, commit the result as `chore: release` and push.
  Once CI passes on `main`, the `release` workflow runs `just publish`.

`just publish` needs `GITHUB_TOKEN` and `CARGO_REGISTRY_TOKEN`. It publishes
each crate version that crates.io lacks, tags it and creates a GitHub release.
A push with nothing new to publish does nothing.

## Clacky

- Scope: `derive` for `clacky-derive`, a module name for the rest.
- Each crate gets its own version, and only when that crate changed, with tags
  `clacky-vX.Y.Z` and `clacky-derive-vX.Y.Z`. `clacky` pins `clacky-derive`
  with `=`, so a derive change bumps both.
- `just compat` checks the MSRV, the no-default-features build (no `tokio` in
  the tree), `cargo package` and the examples. `just size` and `just bench`
  are local helpers.
- If the spell check flags a real word, add it to `spellcheck.dic`. It builds
  against libclang: on Linux install `libclang-dev`; on macOS the Xcode command
  line tools have it, but the first build needs
  `RUSTFLAGS="-C link-arg=-Wl,-rpath,/Library/Developer/CommandLineTools/usr/lib" cargo bin cargo-spellcheck --version`.
- `just release` runs `cargo-semver-checks` (pinned like the other tools) to
  pick the bump from the API diff.
