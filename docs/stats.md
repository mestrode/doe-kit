# doe stats

Statistical analysis of a doe-kit pool: what the designs and the
evaluated results say, read-only, before expensive simulations are run.
The tool reports statistics and nothing else — it never filters, never
ranks, never recommends. What to do with the numbers (evaluate more,
narrow the scope, run the simulations) stays with you.

Two entry points share the same code and produce the same report:

- `sampling stats <file>` — the full report, the everyday command.
- the `doe-stats` binary — the same report as `doe-stats <file>`, plus
  one subcommand per section for pools that are not ready for the full
  report:
  - `doe-stats input <file>` — the parameter definitions only; works on
    a fresh pool with no designs and no results.
  - `doe-stats sampling <file>` — the sampling-point statistics only;
    needs at least one design.
  - `doe-stats output <file>` — the evaluated-result statistics only;
    needs at least one evaluated result.

Flags on both: `--json` (one machine-readable JSON line on stdout,
human output suppressed; errors stay human on stderr) and `--verbose`
(the full detail tier; the default is compact). The pool file is never
written, no backup, no `--no-backup`. Exit codes: `0` success, `1`
validation error, `2` usage or I/O error.

The report runs its sections in a fixed order — input, sampling,
output — and the first section that cannot run ends the run with a
validation error (exit 1). So a fresh pool (no designs) fails at the
sampling section, a designed-but-unevaluated pool at the output
section; the subcommands let you inspect the sections that do run.

## An annotated report

```text
input parameters (2):
  x: uniform, range [0, 10], scope [2, 8] (60.0% of range)
  y: gaussian, range [0, 10], mean 5, sigma 2, scope: full range
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
outputs:
a: n=4, min 1.000000, max 4.000000, mean 2.500000, median 2.500000, std 1.290994, min at 1.1, max at 1.4
  max |pearson| 0.998274 (x), max |spearman| 1.000000 (x), max eta2 1.000000 (x)
b: n=4, min 2.000000, max 8.000000, mean 5.000000, median 5.000000, std 2.581989, min at 1.1, max at 1.4
  max |pearson| 0.998274 (x), max |spearman| 1.000000 (x), max eta2 1.000000 (x)
output pairs: 1, max |pearson| 1.000000, max |spearman| 1.000000
```

## Input: the parameter definitions

One line per parameter: distribution, definition range, mean and sigma
for gaussian parameters, and the scope with its share of the range (a
full-range scope is not restated). Definitions cannot fail — the
parameters were validated when the pool was read.

For gaussian parameters the section tracks how much of the untruncated
distribution lies outside the sampling interval. The compact tier warns
when more than 10% of the mass lies outside the scope (the scope is
what sampling actually covers; with a full-range scope the range
figures apply):

```text
  y: gaussian, range [0, 10], mean 8, sigma 2, scope [6, 9] (30.0% of range)
    warning: 46.7% of the probability mass lies outside the scope
```

`--verbose` prints the below/above values against the range and —
when the scope is narrower — against the scope, and never warns:

```text
    lost probability: 15.9% outside the range (0.0% below, 15.9% above)
    lost probability: 46.7% outside the scope (15.9% below, 30.9% above)
```

A nonpositive `sigma` is a validation error (exit 1).

## Sampling: the quality of the design points

The statistics of the `designs` themselves, independent of any
evaluation — this is what to look at right after `plhs` generated new
designs. Every parameter is linearly scaled to `[0, 1]` of its
definition range (the definition range, not the scope), so metrics are
comparable across parameters with different units.

- `min pairwise distance` — the smallest distance between two designs.
  Near zero means near-duplicate parameter sets.
- `mean pairwise distance` / `median pairwise distance` — the typical
  spread of the points; the median is robust against a few extreme
  outliers stretching the mean.
- `mean nearest neighbor` — the average distance to the closest other
  design; low values mean clumping.
- `discrepancy (CL2)` — the centered L2 discrepancy (Hickernell): a
  single uniformity number for the whole pool, directly comparable
  between candidate pools. Run `plhs --explore` with a different
  `--seed` and compare; lower is better.
- `pooled pair r2` — the sum of squared Pearson correlations over all
  parameter pairs: the score `plhs` optimizes and reports as
  `score S`, readable back on the pool. Pairs with an undefined
  correlation count as 0, exactly like the plhs scoring.
- `empty decile slots` — how many of the per-parameter decile bins
  (10 equal-width bins per definition range) hold no sampling point at
  all. Zero means every section of every range is populated.
- `pairs: N, max |pearson| X, max |spearman| Y` — whether any parameter
  pair moves together; the compact summary of the pair list.

Distances are `n/a` for a single design. Correlations are `n/a` below
3 designs or without variance.

`--verbose` adds, per parameter: descriptive statistics of the design
values (n, min, max, mean, median, sample std), the sampling-point
counts per decile plus the outer deciles (first + last decile —
whether the range edges are populated), and the complete pair list
with Pearson and Spearman per parameter pair.

## Output: the evaluated results

Per numeric result variable (any numeric field on a result entry, e.g.
`energy`; non-numeric fields are ignored), the compact tier prints one
summary line and one line naming the strongest relations:

- Coverage and descriptive statistics: `n` (evaluated designs carrying
  the variable) of the pool's designs, min, max, mean, median, sample
  std.
- `min at <design>, max at <design>` — which designs hold the extremes
  (the first evaluated design wins ties). Factual, not a verdict: where
  the interesting results sit, and around which region to narrow the
  scope.
- `max |pearson| X (p), max |spearman| Y (p), max eta2 Z (p)` — the
  strongest input→output relation per metric, each naming the
  responsible parameter. `n/a` per metric when undefined; ties name the
  lexicographically first parameter.

`--verbose` adds, per parameter of each variable:

- Pearson and Spearman correlation between the parameter value and the
  result. Spearman catches monotonic-but-nonlinear trends.
- `eta2` — the correlation ratio: the between-decile share of the
  result's total variance, computed over the 10 decile bins of the
  parameter's definition range. It catches non-monotonic (e.g.
  U-shaped) relations that both correlations read as ~0. `n/a` below 3
  evaluated designs, with fewer than 2 occupied deciles, or without
  result variance.
- The result mean/std inside the current scope vs outside it (the
  outside line is printed only when evaluated designs exist there).
- The result mean per decile of the definition range — the result
  profile along each parameter.

Why eta2 exists — the simio `bowl` function
(`(a−5)² + b² + (c−2.5)² + 0.3ab`) is U-shaped in `a`. On a 40-design
pool both correlations between `a` and `bowl` read as noise while the
relation is strong:

```text
bowl: n=40, min -0.469128, max 31.049847, mean 10.738568, median 8.954926, std 8.836795, min at 1.9, max at 1.1
  max |pearson| 0.635719 (b), max |spearman| 0.644278 (b), max eta2 0.903985 (a)
  a: pearson -0.042462, spearman -0.038462, eta2 0.903985
```

The correlations point at `b` (its linear term); eta2 points at `a`.
Both are correct — they answer different questions.

Finally the output section correlates the result variables with each
other: every pair of result variables gets a Pearson and Spearman
correlation over the designs evaluated in both (n/a below 3 shared
designs or without variance). The compact tier prints one summary line
(the number of pairs and the maximum absolute correlations); `--verbose`
lists every pair with its shared-design count. With fewer than two
result variables the pairs block is omitted.

Validation in the output sections: a pool without `results` is a
validation error (`no results to analyze`), and so is a result name
that matches no design.

## Tiers and JSON

Both entry points have the same two tiers:

- compact (default) — the summaries: the numbers needed to judge the
  pool and the results at a glance.
- `--verbose` — everything: per-parameter blocks, decile counts and
  complete pair lists.

`--json` prints exactly one line on stdout and follows the same
tiering. The object carries `subcommand` (`stats` for the `sampling
stats` wrapper; `input`, `sampling`, `output` or `all` for the binary),
`file`, `evaluated`, and the section objects `input`, `pool`,
`results` and `output_pairs`. The compact tier prunes the detail lists
(each result drops its per-parameter entries; the pool and the output
pairs carry `n_pairs` instead); `--json --verbose` returns the full
object. The JSON facts are tier-independent where possible: the
gaussian `lost_probability` fractions and each result's `extremes`
(`min_design`, `max_design`) and `relations` (`max_abs_pearson`,
`max_abs_spearman`, `max_eta2`, each `{value, parameter}` or `null`)
ride along in both tiers; thresholds (the 10% warning) never appear in
JSON. Per-parameter entries carry `pearson`, `spearman` and `eta2` in
the verbose tier.

```sh
sampling stats --json designs.json >> results.jsonl
```

## Which metric matters for which purpose

All metrics are always reported; the tool never filters or ranks.
Which ones to read first depends on what the designs are for:

| Purpose | Read first |
| --- | --- |
| Surrogate model / response surface | min pairwise distance, discrepancy (CL2), decile counts |
| Sensitivity analysis | pair correlations (Pearson/Spearman), pooled pair r2 |
| Optimization | decile counts, min pairwise distance (coverage plus resolution) |
| Monte-Carlo simulation | discrepancy (CL2), descriptive statistics (representativity) |
| Boundary / robustness tests | outer deciles, min/max per parameter |

Note that an evenly distributed point set is not automatically a good
design for estimating specific effects: classical factorial designs
and variance analyses need orthogonality, model-matrix rank and
D-efficiency — criteria outside doe-kit's scope of space-filling
sampling.

## The workflow around it

```sh
sampling init designs.json      # skeleton pool, edit params by hand
sampling plhs --explore 40 designs.json   # cover the whole surface
simio test designs.json         # evaluate (analytic example functions)
sampling stats designs.json     # judge the pool and the results
# ... narrow `scope` in the JSON, then:
sampling plhs --add 6 designs.json         # refine the interesting region
```

The tool reports the statistics; the judgment — run expensive
simulations or not, where to set the scope for `--add` — stays with
you.

## Where the code lives

- `crates/sensitivity/stats` — the `doe-stats` library: pure typed
  statistics, no I/O, no formatting. `analyze_inputs`,
  `analyze_pool`, `analyze` (per-result statistics) and
  `analyze_result_pairs` are its public API.
- `crates/sensitivity/bin` — the `doe-stats-bin` crate: the
  `doe-stats` binary and the report/JSON formatting library.
- `crates/sampling/bin` — the `sampling stats` wrapper (the full
  report through the same formatting library).
