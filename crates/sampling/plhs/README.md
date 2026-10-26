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

### Batching

Large runs are placed in chunks of at most `solvable_batch(p)` new designs,
the largest batch whose per-step bin cross product `b^p` fits the greedy
fallback's budget of 10,000,000 combinations — `3162` designs at 2
parameters, `5` at 10, and `1` at 24 and beyond: the standard-LHS loop of
one design per chunk, which works for any parameter count. Every chunk
draws its candidates from the empty bins of the final `n + m` partition —
re-evaluated against the designs earlier chunks placed — selects
(exact up to the cap, greedy up to the cross-product budget), and appends.
When a chunk's empty bins alone would exceed the budget, its candidate set
is restricted to a random subset of `solvable_batch(p)` empty bins per
parameter, so a chunk search always fits. A batched run therefore always
succeeds where a single shot would hit the greedy wall (50 parameters x 6
empty bins ~ 8.1e38 combinations), and the finished pool keeps one design
per bin per parameter. A single chunk keeps the historical single-shot
behaviour, draw for draw. Batched runs report the pooled score over all
pairs and all new designs, and the aggregate strategy: `exact` or `greedy`
when every chunk agrees, `mixed` otherwise.

### Polish

Chunks (and greedy steps) are optimized one at a time, so the finished
assignment is improvable even when every chunk ran exact: greedy chunks
drift, and batched exact chunks are per-chunk optimal, never jointly.
`augment()` therefore runs a deterministic post-placement refinement
pass after the chunk loop, before any values materialize: a greedy swap
descent whose move exchanges the values of two new designs within one
parameter. The per-parameter value multisets — and with them the Latin
Hypercube property — are structurally preserved by every move. A swap in
parameter `j` changes only the pairs involving `j`, each scored in O(1)
from running raw sums. Sweeps scan all candidate swaps in lexicographic
order and apply strictly improving ones immediately; the pass stops at
the first sweep without an improvement (convergence) or when a bounded
evaluation budget is exhausted (up to 32 sweeps, at least one, no
randomness anywhere). A single-chunk exact run skips the pass: the exact
enumeration already searched the whole assignment space, of which the
descent's swap space is a subset, so it is a provable no-op there. The
pass is the quality stage for the deficit paths of the chunked selection:
a single greedy chunk that drifted at 2 parameters and 1000 designs
polishes from a pooled score of 0.386 to ~1e-13 in milliseconds, and the
forced one-design-per-chunk loop at 50 parameters gains ~20% score.

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