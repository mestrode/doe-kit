# Contributing to doe-kit

Thanks for your interest in contributing!

## Workflow

All changes go through pull requests. Direct commits to `main` are not accepted.

1. Create a branch from `main`
   (or from the PR you're stacking on, if your change depends on one).
2. Make your changes, keeping behavior deterministic:
   same input + same `--seed` must always produce the same output.
3. Update or add golden fixtures (`expected_*.json`) when behavior changes.
4. Open a pull request against `main`.

## Repository layout

This is a Cargo workspace. Crates are organized by domain:

- `crates/doe-kit` — the `doe` CLI crate (binary name: `doe`)
- `crates/doe-types` — shared pool data types: parameters, distributions,
  designs, results, and the workspace-wide error types
- `crates/doe-pool` — the pool file format crate (`doe-pool`): read,
  parse, backup and write the JSON design pool
- `crates/sampling/bin` — the `sampling` CLI crate (`doe-sampling`, binary
  name: `sampling`; one subcommand per sampling method)
- `crates/sampling/*` — sampling method crates (`plhs`, ...)
- `crates/metamodel/*` — surrogate/metamodel crates (planned)
- `crates/sensitivity/bin` — the `doe-stats` CLI crate
  (`doe-stats-bin`, binary name: `doe-stats`; the report formatting
  library plus the binary)
- `crates/sensitivity/*` — analysis of evaluated results (`doe-stats`,
  pure statistics computation, used by `sampling stats` and the
  `doe-stats` binary)
- `crates/simulation/*` — simulation input/output crates (`doe-simio`,
  binary name: `simio`; `test` evaluates the analytic example
  functions, `export`/`exec`/`import` for external simulators are
  planned)

A sampling method lands as a lib crate under `crates/sampling/<method>` plus
a subcommand in `crates/sampling/bin`; method crates are lib-only.

A new crate must be registered in the `members` list of the root `Cargo.toml`.
Each crate carries its own version (independent semver) and its own release
cadence; publication to crates.io is manual and per crate.

## Development setup

- Rust edition 2024, MSRV 1.85. Use a recent stable toolchain.
- Build and test:

  ```
  cargo build
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --check
  ```

## Design constraints

- Keep the workspace dependency-light.
- RNG in sampling code is ChaCha8 with an explicit `--seed` (default 42);
  never introduce nondeterminism or unseeded randomness.
- The `sampling plhs` CLI contract: `sampling plhs --add N file` /
  `sampling plhs --explore N file` with options `--seed`, `--output`,
  `--no-backup`, `--cap`, `--no-polish`, `--json`. Exit codes: 0 success,
  1 validation error, 2 I/O error. Keep these stable.
- Exactly one of `--add` / `--explore` must be provided.
- `sampling stats` reports statistics only: it writes no files and makes
  no recommendations; the judgment stays with the user. Its report is
  the full `doe-stats` report: the sections run in order (input,
  sampling, output) and the first section that cannot run is a
  validation error (exit 1) — result names are validated only where
  results are read.
- The `doe-stats` CLI contract: `doe-stats input|sampling|output file`
  plus the bare `doe-stats file` for the full report; statistics-only,
  the pool file itself is read-only (no backups, no `--no-backup`),
  flags `--json`/`--verbose`/`--html`, exit codes 0 success / 1
  validation error / 2 usage or I/O error, unknown subcommand is a
  usage error. Keep these stable.
- The input section is two-tier for Gaussian parameters: the compact
  tier warns when the untruncated mass outside the scope exceeds 10%
  (strictly above; the scope is the sampling interval), `--verbose`
  prints the below/above values against the range and the scope and
  never warns. The JSON `input` object carries the `lost_probability`
  facts in both tiers — thresholds never appear in JSON. A Gaussian
  `sigma` must be greater than zero (validation error).
- The output section reports per result variable: the extreme designs
  (`min at` / `max at`; the first evaluated design wins ties), and one
  relation maxima line `max |pearson| X (p), max |spearman| Y (p),
  max eta2 Z (p)` — `n/a` per metric when undefined, ties name the
  lexicographically first parameter. `eta2` is the correlation ratio
  over the 10 decile bins of the parameter's definition range (the
  between-bin share of the result's total variance; undefined below 3
  evaluated designs, with fewer than 2 occupied bins, or without result
  variance) — it catches non-monotonic relations the correlations miss.
  The JSON `results` objects carry `extremes` and `relations` in both
  tiers; `--verbose` adds `parameters[].eta2`.
- `--json` on a subcommand means one machine-readable line on stdout,
  human output suppressed, errors still human on stderr. Every
  sampling-method subcommand gets this flag.
- `--verbose` on a subcommand switches the output to the full detail
  tier; the default output is compact. Subcommands with detail depth
  get this flag, and `--json` follows the same tiering: the default is
  the compact object, `--json --verbose` the full one.
- `--html` on a view writes the interactive single-file report to
  `<stem>-report.html` next to the input file (silently overwritten;
  write failure is an I/O error, exit 2). Combinable with `--json`:
  the machine line stays the only stdout, the report file is written
  additionally. `--verbose` has no effect — the HTML report always
  embeds the full tier. The embedded JavaScript renders only; every
  displayed statistic is computed in Rust. Charts use Plotly.js from
  a CDN, so diagram rendering needs internet; the statistics and
  tables render offline.
- Input files are backed up to `<file>.bak` unless `--no-backup` is given.
- Pool files are read, parsed and written through the `doe-pool` crate; do
  not reimplement pool file handling in another crate or binary.
- New user-facing functionality goes behind subcommands of the `doe` CLI;
  per-crate binaries are transitional.

## Commit style

Small, focused commits. Write commit messages in the imperative mood
("add explore mode", not "added explore mode").

## Reporting issues

Include the version (`doe --version`), the command you ran, the input
JSON (reduced if possible), and the seed. Bugs that break determinism
(same seed, different output) are treated as high severity.
