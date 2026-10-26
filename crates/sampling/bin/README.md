# doe-sampling

The `sampling` command-line tool: Design of Experiments sampling methods,
one subcommand per method. The crate is named `doe-sampling` because
`sampling` is taken on crates.io; the installed binary is `sampling`.

```sh
cargo install doe-sampling
```

Currently available subcommands:

| Subcommand | Purpose |
| --- | --- |
| `init` | create a new pool file with three example parameters (`a`, `b` uniform, `c` gaussian) |
| `plhs` | pooled Latin Hypercube Sampling (the `plhs` crate) |
| `stats` | statistical analysis of the pool: sampling points and evaluated results (the `doe-stats` crate) |

## Creating a new pool: `sampling init`

```sh
sampling init designs.json
```

Writes a skeleton pool with three example parameters — `a` and `b`
uniform over `[0, 10]`, `c` gaussian over `[0, 10]` with mean 5 and
sigma 2 (the input names of the analytic example simulators) — and an
empty `designs` list. Edit `params` by hand, then run `plhs` or `stats`
on the file. The skeleton sets no `scope` (the full parameter ranges
apply) and no `results` (nothing is evaluated yet), so it is
immediately valid. `init` refuses to overwrite an existing file
(validation error, exit 1); there is no `--force`.

## Usage

```sh
sampling plhs --add 2 designs.json        # refine inside the scopes
sampling plhs --explore 3 designs.json    # cover the whole surface
```

Exactly one of `--add` and `--explore` is required; giving both, or
neither, is a validation error.

Options:

| Option | Description |
| --- | --- |
| `--add N` | number of new designs inside the per-parameter scopes |
| `--explore N` | number of new designs uniformly covering each parameter's full definition range (ignores `scope` and `distribution`) |
| `--seed N` | random seed, default 42 (fixed, for reproducible runs) |
| `--output FILE` | output file, default: the input file |
| `--no-backup` | skip writing the backup file |
| `--cap N` | exact-enumeration cap, default 1000000; above it, greedy sequential selection is used |
| `--no-polish` | skip the post-placement refinement pass (swap descent) that runs after the selection on greedy, mixed and batched runs |
| `--json` | print one line of machine-readable JSON instead of the human summary (benchmark and result collection) |

The input file is read, `N` new designs are appended to `designs`,
`new_designs` is overwritten with the new batch, and the result is written
back atomically. A backup of the original input is written to
`<input>.bak` unless `--no-backup` is given. Unknown JSON fields (including
`results`) and key order are preserved.

The two modes form a workflow: run `--explore` first to spread designs over
the whole parameter surface when nothing is known yet, evaluate the
results, then narrow `scope` in the JSON and run `--add` to refine the
interesting sections.

Large runs are placed in chunks automatically: `--explore 500` on a
50-parameter pool works in one invocation (chunks of one design each —
the standard-LHS loop — there), writing one backup, one iteration and one
`--json` line. The pool keeps the Latin Hypercube property of one design
per bin per parameter.

Because chunks (and greedy steps) are optimized one at a time, the
finished assignment is refined afterwards by a deterministic polish pass:
a greedy swap descent that exchanges the values of two new designs
within one parameter, which cannot break the Latin Hypercube property.
It runs on every greedy, mixed and batched run — including batched
runs of exact chunks, which are per-chunk optimal but never jointly —
and is skipped only for single-chunk exact runs, which it provably
cannot improve (the exact enumeration already searched the same
assignment space). The pass stops at convergence (a sweep without an
improving swap) or after a bounded evaluation budget. The human summary
appends `, polished with N swap(s) (was S)` when swaps were applied.

Exit codes: `0` success, `1` validation error, `2` I/O error (missing
subcommand or unknown subcommand: usage error).

## Machine-readable output: `--json`

Both `plhs` and `stats` accept `--json`: instead of the human summary or
report, they print exactly one line of JSON on stdout and suppress all
human output. Errors stay human on stderr with the usual exit codes; the
pool is written and backed up exactly as without the flag.

```sh
sampling plhs --add 2 --json designs.json >> results.jsonl
sampling stats --json designs.json >> results.jsonl
```

The `plhs` object carries `subcommand`, `mode` (`add`/`explore`), `file`,
`seed`, `cap`, `m`, `designs_before`, `designs_after`, `iteration`,
`n_params`, `strategy` (`exact`/`greedy`, or `mixed` when a batched run's
chunks used different strategies), `chunks` (how many chunks the `m` new
designs were placed in, 1 for a single-shot run), `polished` (whether the
post-placement refinement pass ran), `score` and `elapsed_ms`
(the compute phase only: after reading the pool, before writing it).
When the pass ran, `polish_moves`, `polish_score_before` and
`polish_score_after` report what it did (`score` is always the
post-polish score). The
`stats` object carries
`subcommand`, `file`, `evaluated`, `pool` and `results`, mirroring the
human report's numbers. Both commands follow the same tiering as their
human output: `stats --json` prints the compact object (the pool
without its per-parameter entries and pair list, carrying `n_pairs`
instead; each result without its per-parameter entries), `stats --json
--verbose` the full one. This flag is the output convention for every
future sampling-method subcommand, so benchmark runs can be collected
line by line into one file.

## Judging the pool and results before expensive simulations: `sampling stats`

```sh
sampling stats designs.json
```

Read-only statistical analysis of the pool (nothing is written, the input
file is untouched). The report has two tiers: the default is compact —
the pool line, the global sampling metrics, an occupancy and a pair
summary, and one line per result variable; `--verbose` adds the
per-parameter detail and the complete pair list.

Statistics of the sampling points themselves (the `designs`, independent
of any evaluation — this is what to look at right after `plhs` generated
new designs). The pool line opens the report with the total, the
evaluated count and the context number of designs per parameter:

1. Space-filling metrics over all designs in the unit cube (every
   parameter linearly scaled to [0, 1] of its definition range, so
   mixed units stay comparable): the minimal pairwise distance (near
   zero means near-duplicate parameter sets), the mean and median
   pairwise distance (the typical spread; the median is robust against
   a few extreme outliers) and the mean nearest-neighbor distance
   (clumping); all n/a for a single design.
2. The centered L2 discrepancy (CL2, Hickernell) in the same normalized
   space: a single uniformity number for the whole pool, comparable
   between candidate pools — run `--explore` with a different `--seed`
   and compare; lower is better.
3. The pooled pair r2: the sum of squared Pearson correlations over
   all parameter pairs — the score `plhs` optimizes and reports as
   `score S`, readable back on the pool (identical on pools without
   scopes; `plhs` scores the in-scope series). Pairs with an undefined
   correlation count as 0, as in the plhs scoring; n/a when no pair
   correlation is defined at all.
4. Empty decile slots: how many of the per-parameter decile bins
   (10 equal-width bins per parameter definition range) hold no
   sampling point; zero means every section of every range is
   populated.
5. The pair summary: the number of parameter pairs and the maximum
   absolute Pearson and Spearman correlation over all pairs (n/a below
   3 designs or without variance) — one line telling whether any
   parameter pair moves together.

`--verbose` adds the detail behind those summaries:

1. Per parameter: descriptive statistics of the design values (n, min,
   max, mean, median, sample std).
2. Per parameter: sampling-point counts per decile (10 equal-width bins)
   of the definition range — how uniformly the range is covered — plus
   the outer deciles (points in the first + last decile), telling
   whether the range edges are populated.
3. Per parameter pair: Pearson and Spearman correlation between the
   parameter values (n/a below 3 designs or without variance).

Statistics of the evaluated `results` (reported only when `results`
exist). Per numeric result variable (any numeric field on a result
entry, e.g. `energy`), the compact tier prints one summary line:
coverage (how many of the pool's designs are evaluated) and the
descriptive statistics n, min, max, mean, median, sample std.
`--verbose` adds, per parameter:

1. Pearson and Spearman correlation between the parameter value and the
   result (n/a below 3 evaluated designs or without variance —
   Spearman catches monotonic-but-nonlinear trends).
2. The result mean/std inside the current scope vs outside it (printed
   only when evaluated designs exist outside the scope).
3. The result mean per decile (10 equal-width bins) of the definition
   range.

A pool without `results` is no longer an error: the sampling points are
analyzed on their own, so `init` followed by `plhs` and `stats` reports
how well the generated points are distributed and whether they
correlate. The tool reports statistics; the judgment — run expensive
simulations or not, where to set the scope for `--add` — stays with you.
Exit codes as above; a pool with no designs at all, or a result name
that matches no design, is a validation error (exit 1).

### Which metric matters for which purpose

All metrics are always reported; the tool never filters or ranks. Which
ones to read first depends on what the designs are for:

| Purpose | Read first |
| --- | --- |
| Surrogate model / response surface | min pairwise distance, discrepancy (CL2), decile counts |
| Sensitivity analysis | pair correlations (Pearson/Spearman), pooled pair r2 |
| Optimization | decile counts, min pairwise distance (coverage plus resolution) |
| Monte-Carlo simulation | discrepancy (CL2), descriptive statistics (representativity) |
| Boundary / robustness tests | outer deciles, min/max per parameter |

Note that an evenly distributed point set is not automatically a good
design for estimating specific effects: classical factorial designs and
variance analyses need orthogonality, model-matrix rank and
D-efficiency — criteria outside doe-kit's scope of space-filling
sampling.

## JSON format

```jsonc
{
  "params": [
    { "name": "x", "min": 0, "max": 10, "distribution": "uniform" },
    { "name": "y", "min": 0, "max": 10, "mean": 5, "sigma": 2, "distribution": "gaussian" }
  ],
  // optional: narrows each parameter's range; entry per parameter required
  "scope": [
    { "name": "x", "min": 2, "max": 8 },
    { "name": "y", "min": 3, "max": 7 }
  ],
  "designs": [
    { "name": "0.1", "x": 2.1667, "y": 4.4263 },
    { "name": "0.2", "x": 4.8333, "y": 3.0121 }
  ],
  "new_designs": [],
  "results": [
    { "name": "0.1", "energy": 42.1 }
  ]
}
```

- `params`: parameter list. `distribution` defaults to `uniform`;
  `gaussian` (alias `normal`) requires `mean` (alias `nom`) and `sigma`,
  and is truncated to `[min, max]`.
- `scope`: a `name`/`min`/`max` entry per parameter; the distribution is
  inherited from the parameter definition. Missing scope = copy of `params`.
  Used by `--add`; ignored by `--explore` (still validated).
- `designs`: all designs (old + new, appended).
- `new_designs`: the designs added by the last run (overwritten).
- `results`: output values per design name; passed through untouched.

New designs are named `{iteration}.{design}` with `design` starting at 1;
the next iteration is detected from the existing names (maximum found plus
one, 1 when no name matches the pattern).

## Determinism

Candidate values are drawn from a ChaCha8 RNG seeded with `--seed` (default
42). The same input file and seed always produce the same output, verified
by byte-for-byte golden tests in CI — one for `--add` and one for
`--explore`.

## Development

All changes go through pull requests; direct commits to `main` are not
allowed. CI runs `cargo fmt --check`, `cargo clippy -- -D warnings` and
`cargo test` on every pull request.

## License

MIT