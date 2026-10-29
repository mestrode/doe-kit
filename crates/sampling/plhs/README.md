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
parameter** (the Latin Hypercube property) with the best combined objective
is chosen. The primary criterion is the saturated correlation key: the sum
over all parameter pairs of each pair's squared Pearson correlation floored
at the null floor `τ = 1e-2` (|r| ≤ 0.1 counts as null), computed on the
transformed values of the shared in-scope designs plus all new candidates.
The floor caps what null headroom is worth — the search cannot buy real
correlation in one pair by spending null-level headroom in another. Among
exact ties of the key, the lower Audze–Eglais space-filling energy of the
bin-center proxy wins: the new candidates and earlier chunks' placements
sit at their bin centers on the unit cube, the original in-scope designs
at their actual transformed values. The tie-break is budgeted (2e9 distance
terms per chunk): once the budget runs out, the remaining assignments
compare on the key alone, and the run is flagged so the polish pass still
applies. Assignments are enumerated exactly up to the configured cap;
beyond that, a deterministic greedy fallback places designs one at a time
under the same comparator. All computation happens in `[0, 1]` space; real
values are written only in the output.

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
pass after the chunk loop, before any values materialize. Its move
exchanges the values of two new designs within one parameter; the
per-parameter value multisets — and with them the Latin Hypercube
property — are structurally preserved by every move. Sweeps scan all
candidate swaps in lexicographic order and apply strictly improving
ones immediately; the pass stops at convergence or when a bounded
evaluation budget is exhausted (no randomness anywhere). A single-chunk
exact run skips the pass: the exact enumeration already searched the
whole assignment space, of which the descent's swap space is a subset,
so it is a provable no-op there — unless the chunk's AE tie-break budget
ran out mid-search, which voids the joint-optimality proof and re-enables
the pass.

The pass descends a combined objective in two phases:

- **Phase 1 (correlation)** greedily descends the pooled pair r², the
  unfloored form of the selection's saturated key. A swap in parameter `j`
  changes only the pairs involving `j`, each scored in O(1) from running
  raw sums.
- **Phase 2 (space-filling)** descends the Audze–Eglais potential
  energy of the active point set (the designs in scope for every
  parameter plus the new designs, on the unit cube) under a per-pair
  r² guard. This fixes what r² cannot see: an X-shaped pair scatter —
  two crossing lines — has zero Pearson AND zero Spearman on every
  pair while being perfectly dependent, so inside the r² ≈ 0 manifold
  the correlation objective is flat and the landing point is
  trajectory-determined, not quality-determined. The guard runs an
  ascending ladder of per-pair r² floors (1e-9, 1e-4, 1e-2, tightest
  first, each tier to its own convergence): below the active floor the
  two criteria trade freely, above it correlation dominates outright.
  A tolerance tighter than the O(1/m) r² steps of the swap landscape
  would freeze the search inside the null-correlation manifold —
  exactly where the X lives — so the loosest floor (r² ≤ 1e-2 per
  pair, |r| ≤ 0.1) instead spends practically null correlation
  headroom on genuine space-filling. Phase 2 keeps its own evaluation
  budget, counted in distance terms and shared across the ladder.

The pass is the quality stage for the deficit paths of the chunked
selection: a single greedy chunk that drifted at 2 parameters and
1000 designs polishes from a pooled score of 0.386 to ~1e-13 in
milliseconds, the forced one-design-per-chunk loop at 50 parameters
gains ~20% score, and a fresh-pool exploration (100 designs, 3
parameters) no longer lands on the X-shaped zero-correlation minimum
at all: the saturated key plus AE tie-break place it at a pooled r² of
~5e-4 — the pooled-r²-only search landed at 0.72 — and the pass takes
that to ~6e-5 with an AE energy of 3.8 and a minimum pairwise distance
of ~0.20 on the unit cube.

`explore()` runs the same machinery with every parameter treated as
uniform over its full definition range `[min, max]`: scopes and non-uniform
distributions are ignored for placement, the bins span the whole range, and
all existing designs take part in the binning (every value lies inside the
definition range by definition). The result is a uniform, low-correlation
enhancement of the whole surface instead of a specific section.

`polish_pool()` exposes the refinement pass standalone for whole-pool
polishing (the `sampling polish` subcommand): it maps the pool through
the same `full_surface` view — every parameter on its definition range,
the normalization the statistics use — and marks which designs are
movable. The unevaluated designs descend under the combined objective
with the evaluated ones as fixed context; the pass, its budgets and
its τ-guard ladder are exactly the ones described above.

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