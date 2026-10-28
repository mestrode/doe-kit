# doe-stats output

The output section of the doe statistics: what the evaluated results
say. Per result variable it reports the descriptive statistics, the
designs holding the extremes, and the strongest relation of each
kind between the result and the parameters — and it correlates the
result variables with each other. The section answers "which
parameter drives which output" with numbers, never with a verdict:
it does not filter, rank or recommend.

Two entry points share the same code and produce the same section:

- `doe-stats output <file>` — the output section alone. Needs at least
  one evaluated result.
- the full report — bare `doe-stats <file>` or `sampling stats <file>`
  — prints the output section last, after input and sampling. A
  designed-but-unevaluated pool fails the full report here
  (`no results to analyze`, exit 1); the subcommand isolates the
  view.

Flags: `--json` (one machine-readable JSON line on stdout, human
output suppressed) and `--verbose` (the full detail tier). Exit codes:
`0` success, `1` validation error, `2` usage or I/O error. The pool
file is read-only.

Result names are validated wherever results are read — this section
and the bare report, not the input/sampling views.

## An annotated report

Compact tier (default). Four designs, one parameter `x`, one result
variable `y` with values `5, 2, 2, 5` at `x = 1, 4, 6, 9` — a
U-shape:

```text
outputs:
y: n=4, min 2.000000, max 5.000000, mean 3.500000, median 3.500000, std 1.224745, min at 1.2, max at 1.1
  max |pearson| 0.000000 (x), max |spearman| 0.000000 (x), max eta2 1.000000 (x)
```

Both correlations read the U-shape as exactly zero — there is no
monotonic trend to find. eta2 sees it fully: all of the result's
variance sits between the decile bins of `x`, none within them.
That contrast is the reason the relation line carries three metrics.

With a second result variable `z = 2y`, the section ends with the
output-pairs summary:

```text
output pairs: 1, max |pearson| 1.000000, max |spearman| 1.000000
```

## Result variables

A result variable is any numeric field on a result entry (e.g.
`energy`); non-numeric fields are silently ignored. Variables are
reported in alphabetical order, one block each:

- `n` — the evaluated designs carrying the variable, of the pool's
  total; a design without the field does not count.
- `min, max, mean, median, std` — the descriptive statistics of the
  variable (sample std, n-1 denominator).
- `min at <design>, max at <design>` — which designs hold the
  extremes; the first evaluated design wins ties. Factual, not a
  verdict: where the interesting results sit, and around which
  region to consider narrowing the scope.

## The strongest relations

The line under each variable, always printed, names the strongest
input-to-output relation per metric:

```text
  max |pearson| 0.998274 (x), max |spearman| 1.000000 (x), max eta2 1.000000 (x)
```

- `max |pearson|` — the strongest linear relation.
- `max |spearman|` — the strongest monotonic relation (catches
  monotonic-but-nonlinear trends).
- `max eta2` — the strongest relation of any shape (see below).

Each metric prints `n/a` on its own when no parameter defines it;
ties name the lexicographically first parameter. The three metrics
answer different questions — a linear driver maximizes pearson, a
monotonic one spearman, a non-monotonic one eta2.

## eta2 — the correlation ratio

`eta2` is the between-bin share of the result's total variance,
computed over the 10 decile bins of the parameter's definition range
(evaluated designs only): 1.0 when the decile means explain
everything, toward 0 when the result is flat along the parameter.
It catches non-monotonic (U-shaped) relations that both correlations
read as ~0. It is `n/a` below 3 evaluated designs, with fewer than 2
occupied deciles, or without result variance.

The built-in counterexample is the simio `bowl` function
(`(a−5)² + b² + (c−2.5)² + 0.3ab`), U-shaped in `a`. On a 40-design
pool:

```text
bowl: n=40, min -0.469128, max 31.049847, mean 10.738568, median 8.954926, std 8.836795, min at 1.9, max at 1.1
  max |pearson| 0.635719 (b), max |spearman| 0.644278 (b), max eta2 0.903985 (a)
```

The correlations point at `b` (its linear term); eta2 points at `a`.
Both are correct — they answer different questions.

## The verbose per-parameter blocks

`--verbose` adds, per parameter of each variable:

```text
  x: pearson 0.000000, spearman 0.000000, eta2 1.000000
     in scope: 4 evaluated, mean 3.500000, std 1.224745
     outside scope: none evaluated
     deciles: [5.000000, -, -, 2.000000, -, 2.000000, -, -, 5.000000, -]
```

- the three relation metrics per parameter (`n/a` per metric; the
  line gains the suffix `n < 3, no variance or too few occupied
  deciles` when any of them is undefined).
- the result mean/std inside the parameter's scope vs outside it
  (the outside line is printed only when evaluated designs exist
  there).
- the result mean per decile of the definition range — the result
  profile along the parameter; empty deciles print `-`.

## Output pairs

Finally the section correlates the result variables with each other:
every pair gets a Pearson and a Spearman correlation over the designs
evaluated in both. `n/a` below 3 shared designs or without variance;
with fewer than two result variables the pairs block is omitted
entirely. The compact tier prints the summary line (number of pairs,
maximum absolute correlations); `--verbose` lists every pair with its
shared-design count:

```text
  a vs b: pearson 1.000000, spearman 1.000000 (n 4)
```

## Validation

- a pool without `results` (or with an empty array) is a validation
  error: `no results to analyze`, exit 1.
- a result name that matches no design: `result 'x' does not match
  any design`, exit 1.
- result entries carrying no numeric value at all: validation error.
- non-numeric fields are ignored, not an error.

## Tiers and JSON

`--json` prints exactly one line on stdout and follows the same
tiering. The output view carries `subcommand` (`output`), `file`,
`evaluated`, the `results` array and the `output_pairs` object; the
full report nests them next to `input` and `pool` (with the wrapper's
`stats` label). One result object, compact tier:

```json
{
  "name": "y",
  "n": 4,
  "total": 4,
  "min": 2.0,
  "max": 5.0,
  "mean": 3.5,
  "std": 1.224744871391589,
  "median": 3.5,
  "extremes": {
    "min_design": "1.2",
    "max_design": "1.1"
  },
  "relations": {
    "max_abs_pearson": {"value": 0.0, "parameter": "x"},
    "max_abs_spearman": {"value": 0.0, "parameter": "x"},
    "max_eta2": {"value": 1.0, "parameter": "x"}
  }
}
```

- `extremes` (`min_design`, `max_design`) and `relations`
  (`max_abs_pearson`, `max_abs_spearman`, `max_eta2`, each
  `{value, parameter}` or `null`) ride along in both tiers — the
  facts are tier-independent.
- the compact tier drops each result's `parameters` array; verbose
  carries it with `pearson`, `spearman` and `eta2` per parameter.
- the compact `output_pairs` object replaces its list with `n_pairs`,
  keeping the maxima; verbose carries every pair.

## The workflow around it

```sh
sampling plhs --explore 40 designs.json   # cover the whole surface
simio test designs.json         # evaluate (or your own simulation)
doe-stats output designs.json  # what the results say, which parameter drives what
# ... narrow `scope` in the JSON, then `plhs --add` refines the interesting region
```

The section reports the statistics; the judgment — which parameter
matters enough to act on, where to set the scope — stays with you.

## Where the code lives

- `crates/sensitivity/stats` — the `doe-stats` library: pure typed
  statistics, no I/O, no formatting. `analyze` (per-result statistics,
  extremes, relations, eta2) and `analyze_result_pairs` are its
  output API.
- `crates/sensitivity/bin` — the `doe-stats-bin` crate: `human_output`
  and `json_output` (the tiering is presentation, done here).
- `crates/sampling/bin` — the `sampling stats` wrapper (the full
  report through the same formatting library).
