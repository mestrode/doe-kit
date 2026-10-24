# doe-kit

A Design of Experiments toolset. Crates work together through a shared pool
model of experiment designs; the `doe` command-line tool ties them together.

## Crates

- `doe-kit` — the `doe` command-line tool
- `doe-types` — shared pool/design data types (placeholder; types land with
  the sampling crate)
- `crates/sampling/*` — sampling methods, starting with `plhs`, pooled Latin
  Hypercube Sampling (in review)

## Development

Rust edition 2024, MSRV 1.85.

```
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```
