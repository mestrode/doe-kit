# doe-stats sampling

The sampling section of the doe statistics: what the pool's design
points say about themselves, independent of any evaluation. This is
the quality gate before expensive simulations — run it right after
`plhs` generated new designs. The section reports statistics and
nothing else; whether a pool is good enough for its purpose stays
with you.

Two entry points share the same code and produce the same section:

- `doe-stats sampling <file>` — the sampling section alone. Needs at
  least one design; an empty pool is a validation error (`no designs
  to analyze`, exit 1).
- the full report — bare `doe-stats <file>` or `sampling stats <file>`
  — prints the sampling section second, after input. A fresh pool
  fails the full report here; the subcommand isolates the view.

Flags: `--json` (one machine-readable JSON line on stdout, human
output suppressed) and `--verbose` (the full detail tier). Exit codes:
`0` success, `1` validation error, `2` usage or I/O error. The pool
file is read-only. Result names are not validated here — results are
not read at all.

## An annotated report

Compact tier (default), a pool with four designs over two parameters:

```text
pool: 4 designs, 4 evaluated, 2 parameters (2 designs per parameter)
sampling points:
  min pairwise distance: 0.212132
  mean pairwise distance: 0.358069
  median pairwise distance: 0.326862
  mean nearest neighbor: 0.212132
  discrepancy (CL2): 0.344545
  pooled pair r2: 0.981744
  empty decile slots: 12 of 20
  pairs: 1, max |pearson| 0.990830, max |spearman| 1.000000
```

`--verbose` adds, per parameter, the descriptive statistics of the
design values and the decile occupancy:

```text
  x: n=4, min 1.000000, max 9.000000, mean 4.750000, median 4.500000, std 3.500000
     decile counts: [1, 0, 1, 0, 0, 1, 0, 0, 1, 0], outer deciles: 2
```

and the complete pair list:

```text
  x vs y: pearson 0.990830, spearman 1.000000
```

## The pool line

`pool: N designs, K evaluated, P parameters (D designs per parameter)`
— the totals and the ratio of designs to parameters, `D` as an
integer when it divides evenly, one decimal otherwise. The ratio is
the row-count context: how many points carry each parameter's
dimension.

## The unit cube

Every metric below is computed in the unit cube: each parameter is
linearly scaled to `[0, 1]` of its definition range — the definition
range, not the scope. Distances and discrepancy are therefore
comparable across parameters with different units and different
range widths, and the scope setting of any parameter does not
distort them.

## The metrics

- `min pairwise distance` — the smallest distance between two
  designs. Near zero means near-duplicate parameter sets.
- `mean pairwise distance` / `median pairwise distance` — the typical
  spread of the points; the median is robust against a few extreme
  outliers stretching the mean.
- `mean nearest neighbor` — the average distance to the closest other
  design; low values mean clumping.
- `discrepancy (CL2)` — the centered L2 discrepancy (Hickernell): a
  single uniformity number for the whole pool. Directly comparable
  between candidate pools — run `plhs --explore` with a different
  `--seed` and compare; lower is more uniform. For scale: a single
  point at the center of the unit cube scores `sqrt(1/12) ≈ 0.289`,
  two points at opposite corners `sqrt(1/3) ≈ 0.577`.
- `pooled pair r2` — the sum of squared Pearson correlations over all
  parameter pairs: the score `plhs` optimizes and reports as
  `score S`, readable back on the pool. Pairs with an undefined
  correlation contribute 0, exactly like the plhs scoring. Identical
  to the plhs score on pools without scopes (Pearson is invariant to
  the linear scope transform, but plhs scores the in-scope series).
- `empty decile slots` — how many of the `P * 10` decile bins (10
  equal-width bins per definition range) hold no sampling point at
  all. Zero means every section of every range is populated.
- `pairs: N, max |pearson| X, max |spearman| Y` — the compact summary
  of the pair list: whether any parameter pair moves together, and
  how strongly.

Distances are `n/a (single design)` for a single design. Correlations
and `pooled pair r2` are `n/a` below 3 designs, with a single
parameter, or without variance in any pair.

## What the verbose tier adds

Per parameter (in pool order):

- the descriptive statistics of the design values: `n`, `min`, `max`,
  `mean`, `median`, sample `std`.
- the sampling-point count per decile of the definition range plus
  the outer deciles — the sum of the first and last decile counts,
  whether the range edges are populated.

And the complete pair list: every parameter pair with its Pearson and
Spearman correlation (`n/a (n < 3 or no variance)` per pair when
undefined).

## JSON

`--json` prints exactly one line on stdout and follows the same
tiering. The sampling view carries `subcommand` (`sampling`), `file`,
`evaluated` and the `pool` object; the full report nests the same
`pool` object next to `input`, `results` and `output_pairs` (the
`sampling stats` wrapper keeps its `stats` label). Compact pool
object:

```json
{
  "subcommand": "sampling",
  "file": "designs.json",
  "evaluated": 4,
  "pool": {
    "total": 4,
    "min_pairwise_distance": 0.212132,
    "mean_pairwise_distance": 0.358069,
    "median_pairwise_distance": 0.326862,
    "mean_nearest_neighbor": 0.212132,
    "discrepancy_cl2": 0.344545,
    "pooled_pair_r2": 0.981744,
    "empty_decile_slots": 12,
    "max_abs_pearson": 0.99083,
    "max_abs_spearman": 1.0,
    "n_pairs": 1
  }
}
```

- compact: the `parameters` array and the `pairs` list are pruned, the
  global metrics and maxima stay, and the pair count rides along as
  `n_pairs`.
- `--verbose`: the full object — every parameter with its
  descriptive statistics, `deciles` and `outer_deciles`, and every
  pair with `pearson` and `spearman` (or `null` when undefined).

## Validation

- an empty pool (`no designs to analyze`) is a validation error,
  exit 1.
- a design whose value count differs from the parameter count is a
  validation error.
- results are not read: result names play no role here.

## The workflow around it

```sh
sampling plhs --explore 40 designs.json   # cover the whole surface
doe-stats sampling designs.json  # judge the points before evaluating anything
# compare seeds: repeat with --seed S, watch discrepancy (CL2) and pooled pair r2
# ... evaluate, then doe-stats output judges the results
```

The section reports the statistics; the judgment — sample more, try
another seed, or proceed to the simulations — stays with you.

## Where the code lives

- `crates/sensitivity/stats` — the `doe-stats` library: pure typed
  statistics, no I/O, no formatting. `analyze_pool` with `PoolStats`,
  `ParameterPoolStats` and `PairCorrelation` are its sampling API
  (distances, centered L2 discrepancy, deciles, pair correlations).
- `crates/sensitivity/bin` — the `doe-stats-bin` crate:
  `human_sampling` and `json_sampling` (the tiering is presentation,
  done here).
- `crates/sampling/bin` — the `sampling stats` wrapper (the full
  report through the same formatting library).
