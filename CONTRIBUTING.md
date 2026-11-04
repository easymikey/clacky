# Contributing

## Set up

Run `just hooks` once per clone. It points git at `.githooks/`: a commit runs
the format check and clippy, and a commit message that is not a Conventional
Commit is refused. `just` itself comes from `cargo bin just`.

## Before a pull request

Run `cargo bin just`. It runs every check CI runs: format, spelling, clippy,
tests, docs, examples, the dependency checks, the MSRV build, the
no-default-features build and `cargo package`. Format code with
`cargo bin just fmt`. If the spell check flags a real word, add it to
`spellcheck.dic`. The spell check builds against libclang: on Linux install
`libclang-dev`; on macOS the Xcode command line tools have it, but the first
build needs `RUSTFLAGS="-C link-arg=-Wl,-rpath,/Library/Developer/CommandLineTools/usr/lib" cargo bin cargo-spellcheck --version`.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`.

- `feat`: a new feature. Becomes "Added" in the changelog and bumps the minor version.
- `fix`: a bug fix. Becomes "Fixed" and bumps the patch version.
- `refactor`, `perf`, `revert`: become "Changed".
- `docs`, `test`, `build`, `ci`, `chore`, `style`: kept out of the changelog.

The scope is optional: `derive` for `clacky-derive`, a module name for the rest.
Mark a breaking change with `!` (`feat!: ...`) or a `BREAKING CHANGE:` footer.

## Releases

Commits pile up on `main`, and every push there runs the checks. A release
happens when the maintainer decides, in one of two ways with the same result:

- Run the `release` workflow from the Actions tab. It runs `release-plz update`,
  commits `chore: release` to `main`, then publishes.
- Run `just release` locally. It runs `release-plz update` and shows the diff.
  Commit it as `chore: release` and push; once CI passes on `main`, the
  `release` workflow publishes.

Publishing means: each crate version that crates.io lacks is published, tagged
(`clacky-vX.Y.Z`, `clacky-derive-vX.Y.Z`) and gets a GitHub release. A push with
nothing new to publish does nothing.

Each crate gets its own version, and only when that crate changed. `clacky`
pins `clacky-derive` with `=`, so a derive change bumps both.

To check that a release would package and publish, run `cargo bin just release-check`.
