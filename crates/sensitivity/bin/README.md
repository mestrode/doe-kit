# doe-stats-bin

The `doe-stats` command-line tool and its presentation library:
the human-readable report and the machine-readable JSON line for
Design of Experiments pool statistics. The binary is statistics-only —
it reads a pool, prints statistics, never writes. The computations live
in the pure `doe-stats` lib crate (`crates/sensitivity/stats`); this
crate only formats.

The package is named `doe-stats-bin` because the lib crate `doe-stats`
owns the name inside the workspace. The binary itself is called
`doe-stats`.

## Usage

```sh
doe-stats designs.json          # the full report: all three sections
doe-stats input designs.json    # the parameter definitions only
doe-stats sampling designs.json # the sampling-point statistics only
doe-stats output designs.json   # the evaluated-result statistics only
```

Bare `doe-stats <file>` prints the full report — the same output as
`sampling stats`. The sections run in order (input, sampling, output)
and the first one that cannot run ends the run with a validation error
(exit 1):

- `input` needs only the definitions and works on a fresh pool (no
  designs, no results).
- `sampling` needs at least one design; a result name that matches no
  design is only an error where results are read (output, bare).
- `output` needs at least one evaluated result.

So a fresh pool fails at the sampling section, a designed-but-unevaluated
pool at the output section — with the subcommands you can inspect the
sections that do run.

Flags: `--json` (one machine-readable line on stdout, human output
suppressed; `--verbose` widens it to the full tier) and `--verbose`
(the full human detail tier; the default is compact). Exit codes: 0
success, 1 validation error, 2 usage or I/O error. The input file is
read-only; no backups, no writes, no `--no-backup`.

## The sections

Input (works on a fresh pool): one line per parameter — distribution,
range, mean and sigma for gaussian, and the scope with its share of the
range; a full-range scope is not restated. For Gaussian parameters the
section also tracks how much of the untruncated distribution lies
outside the sampling interval: the compact tier warns under a
parameter whose mass outside the scope exceeds 10% (the scope is what
sampling covers; with a full-range scope the range figures apply),
`--verbose` prints the below/above values against the range and — when
the scope is narrower — against the scope, and never warns. The JSON
input object carries the `lost_probability` facts in both tiers (no
threshold in JSON; the scope keys are omitted for a full-range scope).
A nonpositive `sigma` is a validation error (exit 1).

```text
input parameters (2):
  x: uniform, range [0, 10], scope [2, 8] (60.0% of range)
  y: gaussian, range [0, 10], mean 5, sigma 2, scope: full range
```

Verbose adds the values; the compact tier warns instead:

```text
    lost probability: 1.2% outside the range (0.6% below, 0.6% above)
    lost probability: 31.7% outside the scope (15.9% below, 15.9% above)
    warning: 31.7% of the probability mass lies outside the scope
```

Sampling (verbatim the `sampling stats` sampling-points block): the
pool line with total, evaluated count and designs per parameter, the
space-filling metrics (pairwise distances, nearest neighbor), the CL2
discrepancy, the pooled pair r2, decile occupancy, and the parameter
pair summary; `--verbose` adds per-parameter descriptive statistics,
decile counts and every parameter pair. Requires at least one design.

Output (verbatim the `sampling stats` results block) plus the
output↔output correlations: every pair of result variables gets a
Pearson and Spearman correlation over the designs evaluated in both
(n/a below 3 shared designs or without variance; with fewer than two
result variables the pairs block is omitted). Each result variable's
summary line names the designs holding its minimum and maximum
(`min at`, `max at`; ties keep the first evaluated design), and one
line below it names the strongest input→output relations — the
maximum absolute Pearson and Spearman correlation and the maximum
eta2 over the parameters, each with the responsible parameter (`n/a`
per metric when undefined; ties name the lexicographically first
parameter). eta2 (the correlation ratio) is the between-decile share
of the result's variance: it catches non-monotonic, e.g. U-shaped,
relations that both correlations read as ~0.

```text
outputs:
a: n=4, min 1.000000, max 4.000000, mean 2.500000, median 2.500000, std 1.290994, min at 1.1, max at 1.4
  max |pearson| 0.998274 (x), max |spearman| 1.000000 (x), max eta2 1.000000 (x)
b: n=4, min 2.000000, max 8.000000, mean 5.000000, median 5.000000, std 2.581989, min at 1.1, max at 1.4
  max |pearson| 0.998274 (x), max |spearman| 1.000000 (x), max eta2 1.000000 (x)
output pairs: 1, max |pearson| 1.000000, max |spearman| 1.000000
```

`--verbose` prints the per-parameter detail of each result (the
correlation lines gain `eta2`) and every output pair:

```text
  x: pearson 0.998274, spearman 1.000000, eta2 1.000000
  a vs b: pearson 1.000000, spearman 1.000000 (n 4)
```

## JSON

`--json` prints exactly one line: `subcommand` (`input`, `sampling`,
`output`, or `all` for the bare full report — the `sampling stats`
wrapper keeps its `stats` label), `file`, `evaluated`, and the section
objects `input` (single tier: the parameter definitions with their
scope fractions), `pool`, `results` and `output_pairs`. Each result
object carries `extremes` (`min_design`, `max_design`) and `relations`
(`max_abs_pearson`, `max_abs_spearman`, `max_eta2`, each
`{value, parameter}` or `null`) in both tiers. The latter three
follow the tiering: the default is the compact object (the pool and
the output pairs carry `n_pairs` instead of their detail lists; each
result drops its `parameters`), `--json --verbose` the full one —
including the per-parameter `eta2`.
