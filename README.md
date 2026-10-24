# doe-kit

A Design of Experiments toolset. Crates work together through a shared pool
model of experiment designs; the `doe` command-line tool ties them together.

## Crates

- `doe-kit` — the `doe` command-line tool
- `doe-types` — shared pool/design data types (placeholder; types land with
  the sampling crate)
- `doe-sampling` — the `sampling` command-line tool, one subcommand per
  sampling method (`crates/sampling/bin`)
- `crates/sampling/*` — sampling method crates (`plhs`, pooled Latin
  Hypercube Sampling)
- `doe-stats` — statistical analysis of evaluated results
  (`crates/sensitivity/stats`), used by `sampling stats`

## Development

Rust edition 2024, MSRV 1.85.

```
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```
