# plhs

Pooled Latin Hypercube Sampling. `plhs` is a Rust library crate, published
on [crates.io](https://crates.io/crates/plhs). The command-line tool lives
in the sibling crate `doe-sampling`: binary `sampling`, subcommand `plhs`.

`plhs` augments a pool of existing experiment designs with new designs that
preserve the Latin Hypercube property inside a per-parameter **scope** (a
sub-range of each parameter's definition range), while keeping the
correlations between all parameter pairs low.

## Install

Library use:

```toml
[dependencies]
plhs = "0.1"
```

## Explore mode

`explore()` is the scoped augmentation's configuration-free counterpart:
every parameter is treated as uniform over its full definition range
`[min, max]`, ignoring scopes and non-uniform distributions, so the new
designs cover the whole surface instead of a specific section. Explore the
whole surface first, then refine sections with `augment()` and a scope.

## Algorithm

Per parameter (scope judgement is per parameter, so the in-scope counts may
differ between parameters):

1. The scope (default: the parameter definition) is mapped to `[0, 1]`
   through the distribution's CDF truncated to the definition range.
2. The transformed range is binned into `n + m` equal-probability-mass bins
   (`n` = in-scope samples, `m` = new designs), so a uniform distribution
   gives equal-width bins and a gaussian gives narrow bins near the mean.
3. The in-scope samples are located in bins; at least `m` bins are always
   empty (pigeonhole).
4. Every empty bin receives a random candidate value drawn uniformly from
   its probability mass (stratified sampling).

Then the assignment of the `m` new designs to **distinct empty bins per
parameter** (the Latin Hypercube property) with the lowest score is chosen.
The score of an assignment is the sum of squared Pearson correlations over
all parameter pairs, computed on the transformed values of the shared
in-scope designs plus all new candidates. Assignments are enumerated
exactly up to the configured cap; beyond that, a deterministic greedy
fallback places designs one at a time. All computation happens in `[0, 1]`
space; real values are written only in the output.

`explore()` runs the same machinery with every parameter treated as
uniform over its full definition range `[min, max]`: scopes and non-uniform
distributions are ignored for placement, the bins span the whole range, and
all existing designs take part in the binning (every value lies inside the
definition range by definition). The result is a uniform, low-correlation
enhancement of the whole surface instead of a specific section.

## Determinism

Candidate values are drawn from a ChaCha8 RNG seeded with an explicit
seed. The same input and seed always produce the same output; the CLI pins
the default seed to 42 and verifies it with byte-for-byte golden tests.

## Development

All changes go through pull requests; direct commits to `main` are not
allowed. CI runs `cargo fmt --check`, `cargo clippy -- -D warnings` and
`cargo test` on every pull request.

## License

MIT