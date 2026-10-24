# Contributing to plhs

Thanks for your interest in contributing!

## Workflow

All changes go through pull requests. Direct commits to `main` are not accepted.

1. Create a branch from `main`
   (or from the PR you're stacking on, if your change depends on one).
2. Make your changes, keeping the CLI and library behavior deterministic:
   same input + same `--seed` must always produce the same output.
3. Update or add golden fixtures (`expected_*.json`) when behavior changes.
4. Open a pull request against `main`.

## Development setup

- Rust edition 2024, MSRV 1.85. Use a recent stable toolchain.
- Build and test:

  ```
  cargo build
  cargo test
  cargo clippy --all-targets -- -D warnings
  cargo fmt --check
  ```

## Design constraints

- The crate must stay dependency-light and publishable as `plhs` on crates.io.
- RNG is ChaCha8 with an explicit `--seed` (default 42); never introduce
  nondeterminism or unseeded randomness.
- CLI contract: `plhs --add N file` / `plhs --explore N file` with options
  `--seed`, `--output`, `--no-backup`, `--cap`. Exit codes: 0 success,
  1 validation error, 2 I/O error. Keep these stable.
- Exactly one of `--add` / `--explore` must be provided.
- Input files are backed up to `<file>.bak` unless `--no-backup` is given.

## Commit style

Small, focused commits. Write commit messages in the imperative mood
("add explore mode", not "added explore mode").

## Reporting issues

Include the version (`plhs --version`), the command you ran, the input
JSON (reduced if possible), and the seed. Bugs that break determinism
(same seed, different output) are treated as high severity.
