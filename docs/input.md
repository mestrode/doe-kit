# doe-stats input

The input section of the doe statistics: what the pool's parameters
are declared to be. One line per parameter — distribution, definition
range, the gaussian's mean and sigma, and the scope with its share of
the range — plus, for gaussian parameters, how much of the declared
distribution the sampling interval actually captures. The section
reports facts about the definitions and nothing else; the warning it
can print is a fixed diagnostic threshold, not a verdict.

Two entry points share the same code and produce the same section:

- `doe-stats input <file>` — the input section alone. This is the one
  view that works on a fresh pool: no designs and no results are
  needed, because only the definitions are read.
- the full report — bare `doe-stats <file>` or `sampling stats <file>`
  — prints the input section first, before sampling and output.

Flags: `--json` (one machine-readable JSON line on stdout, human
output suppressed) and `--verbose` (the full detail tier). Exit codes:
`0` success, `1` validation error, `2` usage or I/O error. The pool
file is read-only. Results are not read at all, so result names are
not validated here.

## An annotated report

Compact tier (default), a pool with two parameters:

```text
input parameters (2):
  x: uniform, range [0, 10], scope [2, 8] (60.0% of range)
  y: gaussian, range [0, 10], mean 5, sigma 2, scope: full range
```

The header counts the parameters. Each line names the distribution,
the definition range, `mean` and `sigma` for a gaussian, and the scope
with its share of the definition range — a full-range scope is not
restated, it is the default.

The compact tier warns under a gaussian parameter when more than 10%
of the probability mass lies outside the scope (4-space indent,
diagnostic only — the exit code stays 0):

```text
  y: gaussian, range [0, 10], mean 8, sigma 2, scope [6, 9] (30.0% of range)
    warning: 46.7% of the probability mass lies outside the scope
```

`--verbose` shows the values of every gaussian parameter instead and
never warns:

```text
    lost probability: 15.9% outside the range (0.0% below, 15.9% above)
    lost probability: 46.7% outside the scope (15.9% below, 30.9% above)
```

The scope line appears only when the scope is narrower than the
definition range; with a full-range scope the range figures are the
scope figures. A centered gaussian on its range loses mass at both
edges: `1.2% outside the range (0.6% below, 0.6% above)`.

## The definition line

- `name` — the parameter name, in pool order.
- `uniform` or `gaussian` — the distribution.
- `range [min, max]` — the definition range: the full surface the
  parameter can cover.
- `mean M, sigma S` — the untruncated gaussian, gaussian parameters
  only.
- `scope [a, b] (P% of range)` or `scope: full range` — the refinement
  interval and its share of the definition range, `P` with one
  decimal.

Definitions cannot fail to analyze: the parameters were validated
when the pool was read (`min < max`, a positive `sigma`, a scope
inside the definition range; violations are validation errors, exit 1,
before any section prints).

## What the scope means

The scope is the interval `plhs --add` samples within — where
refinement happens. The definition range is the full surface, and
`plhs --explore` deliberately ignores both the scope and the
distribution: it covers the whole definition range uniformly, so the
surface is populated before anything is narrowed. The scope share is
printed so you can see at a glance how much of the surface a
refinement run will see.

## Gaussian lost probability

A gaussian parameter declares an untruncated distribution — `mean`
and `sigma` — but values are only ever drawn inside an interval (the
scope, or the definition range when the scope is full). The mass
outside that interval never becomes design values: the realized
sample follows the truncated distribution, and the larger the lost
mass, the more the effective distribution shifts away from the
declared mean and sigma. The section quantifies exactly that loss.

- Compact tier: a warning when strictly more than 10% of the mass
  lies outside the scope. Exactly 10% does not warn. A full-range
  scope uses the range figures — the scope is what sampling actually
  covers. Diagnostic only; never changes the exit code.
- Verbose tier: the below/above values against the range, always, and
  against the scope when it is narrower. Percentages with one decimal.
  Never warns.
- Uniform parameters lose nothing by construction — no extra lines,
  no warning, no JSON object.

Worked example (`mean 8, sigma 2, range [0, 10], scope [6, 9]`): the
mass below the range is 0.0%, above the range 15.9%; below the scope
15.9%, above the scope 30.9%. Total outside the scope: 46.7% — well
over the threshold, hence the compact warning.

## JSON

`--json` prints exactly one line on stdout. The input view carries
`subcommand` (`input`), `file` and the `input` object; in the full
report the same `input` object sits next to `pool`, `results` and
`output_pairs`. The input object is single tier — identical in the
compact and the verbose JSON, because definitions have no detail
depth.

```json
{
  "subcommand": "input",
  "file": "designs.json",
  "input": {
    "parameters": [
      {
        "name": "x",
        "distribution": "uniform",
        "min": 0.0,
        "max": 10.0,
        "scope": [2.0, 8.0],
        "scope_fraction": 0.6
      },
      {
        "name": "y",
        "distribution": "gaussian",
        "min": 0.0,
        "max": 10.0,
        "mean": 5.0,
        "sigma": 2.0,
        "scope_fraction": 1.0,
        "lost_probability": {
          "below_range": 0.0062097,
          "above_range": 0.0062097
        }
      }
    ]
  }
}
```

- `scope` is omitted when the scope is the full definition range;
  `scope_fraction` (0 to 1) is always carried.
- `lost_probability` carries 0-1 fractions in both tiers, with the
  keys `below_range`, `above_range`, `below_scope`, `above_scope`; the
  scope keys are omitted for a full-range scope, the whole object for
  uniform parameters.
- The 10% threshold never appears in JSON — thresholds are
  presentation, the fractions are the facts. There is also no
  parameter-count key (the array length is the count).

## The workflow around it

```sh
sampling init designs.json    # skeleton pool, edit params by hand
doe-stats input designs.json  # check the edited definitions
sampling plhs --explore 40 designs.json   # cover the whole surface
# ... evaluate, then `sampling stats designs.json` judges pool and results
```

The input section is what to run right after hand-editing `params` —
before any sampling exists, the full report would fail at its
sampling section (`no designs to analyze`, exit 1); the input
subcommand inspects the one section that a fresh pool can answer.

## Where the code lives

- `crates/sensitivity/stats` — the `doe-stats` library: pure typed
  statistics, no I/O, no formatting. `analyze_inputs` with
  `InputStats`, `ParameterDefinition` and `LostProbability` are its
  input API.
- `crates/sensitivity/bin` — the `doe-stats-bin` crate:
  `human_input` and `json_input` (the 10% warning threshold lives
  here, presentation-side).
- `crates/sampling/bin` — the `sampling stats` wrapper (the full
  report through the same formatting library).
