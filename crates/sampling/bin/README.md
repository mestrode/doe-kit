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
| `stats` | statistical analysis of the pool: input definitions, sampling points, evaluated results and their correlations (the `doe-stats` crate) |
| `reset` | remove designs from the pool: the unevaluated ones (default) or all of them (`--all`) |
| `polish` | refine the pool's unevaluated designs in place (deterministic swap descent under the combined objective; evaluated designs are fixed context) |

## Creating a new pool: `sampling init`

```sh
sampling init designs.json
```

Writes a skeleton pool with three example parameters — `a` uniform over
`[0, 10]`, `b` uniform over `[-1, 1]`, `c` gaussian over `[0, 5]` with
mean 2.5 and sigma 1 (the input names of the analytic example
simulators) — and an empty `designs` list. Edit `params` by hand, then
run `plhs` or `stats` on the file. The skeleton sets no `scope` (the
full parameter ranges apply) and no `results` (nothing is evaluated
yet), so it is immediately valid. `init` refuses to overwrite an
existing file (validation error, exit 1); there is no `--force`.

## Removing designs: `sampling reset`

```sh
sampling reset designs.json          # remove every design without a result
sampling reset --all designs.json    # start over: remove every design
```

`reset` removes all designs that have no entry in `results` — the
unevaluated ones; every evaluated design and all results entries are
kept untouched. `--all` removes every design together with the whole
`results` array and the stale `new_designs` batch record: the pool
returns to its initial state (parameters only, empty `designs`), so
the next `plhs` run starts at iteration 1 again. The parameters are
never touched. The pool is written back atomically with the usual
`<input>.bak` backup (`--no-backup` skips it); there is no confirmation
prompt — the backup is the safety net. Removing nothing is a success:
the command is idempotent and reports `removed 0 designs`.

The human summary is `removed N designs (K kept, M results)` or, with
`--all`, `removed all N designs and M results`. The `--json` object
carries `subcommand`, `mode` (`unevaluated`/`all`), `file`,
`designs_before`, `designs_after`, `results_before` and
`results_after`.

## Refining a pool: `sampling polish`

```sh
sampling polish designs.json
```

`polish` refines the pool's unevaluated designs in place: the same
deterministic swap descent that runs after `plhs` placement (`--no-polish`
skips it there), applied standalone to the designs that have no entry in
`results`. Evaluated designs are fixed context — they anchor the
correlation structure and their values never move — and are counted in
the summary. Every swap exchanges two values within one parameter
column, so each parameter's value multiset is preserved exactly: no
design escapes its parameter ranges, the pool stays an LHS in the
per-column sense, and only the pairing changes.

The refinement runs on the full definition ranges (`Parameter::full_surface`,
like `--explore`): scopes and distributions are ignored, every parameter
is normalized to the unit cube of its definition range. The reported
`r2` is therefore exactly the pooled pair r2 that `stats` (and
`doe-stats sampling`) computes on the polished pool — the definition-range
normalization the statistics use is the one being optimized.

A run that cannot improve anything is a success without side effects:
fewer than two movable designs or two parameters, or no improving swap,
reports `polished N designs, r2 X -> X, AE A -> A (0 moves)` and writes
nothing — no rewrite, no backup. Otherwise the refined pool is written
back atomically with the usual `<input>.bak` backup (`--no-backup`
skips it). Because the descent has two phases (correlation first, then
space-filling within the τ guard), a finished pass can leave room for
another: running `polish` again continues descending and is also safe;
the no-op arrives at the fixed point.

The human summary is `polished N designs, r2 X -> Y, AE A -> B (K moves)`
where N counts the movable (unevaluated) designs and K the total swaps of
both phases. The `--json` object carries `subcommand`, `file`, `designs`
(movable), `evaluated_skipped` (fixed context count), `moves`, `r2_before`,
`r2_after`, `ae_before`, `ae_after` and `elapsed_ms`.

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
a two-phase greedy swap descent that exchanges the values of two new
designs within one parameter, which cannot break the Latin Hypercube
property. Phase 1 descends the pooled pair correlation score; phase 2
descends the space-filling (Audze–Eglais) energy of the finished point
set under a per-pair r² guard, breaking the zero-correlation X shapes
the correlation score cannot see. The pass runs on every greedy, mixed
and batched run — including batched
runs of exact chunks, which are per-chunk optimal but never jointly —
and is skipped only for single-chunk exact runs, which it provably
cannot improve (the exact enumeration already searched the same
assignment space under the same combined objective) — unless the
chunk's AE tie-break budget ran out mid-search, which voids the
proof and re-enables the pass. Each phase stops at convergence (a sweep without an
improving swap) or after a bounded evaluation budget. The human summary
appends `, polished with N swap(s) (r2 was S, AE was E)` when swaps
were applied (N counts both phases, S is the pooled pair r² and E the
space-filling energy before the pass).

Exit codes: `0` success, `1` validation error, `2` I/O error (missing
subcommand or unknown subcommand: usage error).

## Machine-readable output: `--json`

Both `plhs` and `stats` accept `--json`; `reset` and `polish` do too
(see their sections above). Instead of the human summary or
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
post-placement refinement pass ran), `score`, `ae_energy` (the
space-filling energy of the finished point set; see the `plhs` README)
and `elapsed_ms`
(the compute phase only: after reading the pool, before writing it).
When the pass ran, `polish_moves`, `polish_score_before` and
`polish_score_after` report the phase-1 correlation descent, and
`polish_ae_moves`, `polish_ae_before` and `polish_ae_after` the
phase-2 space-filling descent (`score` is always the
post-polish score, `ae_energy` the post-polish energy). The
`stats` object carries
`subcommand`, `file`, `evaluated` and the four section objects `input`,
`pool`, `results` and `output_pairs`, mirroring the human report's
numbers. Both commands follow the same tiering as their
human output: `stats --json` prints the compact object (the pool
without its per-parameter entries and pair list, carrying `n_pairs`
instead; the output pairs without their pair list; each result without
its per-parameter entries), `stats --json --verbose` the full one. This
flag is the output convention for every future sampling-method
subcommand, so benchmark runs can be collected line by line into one
file.

## Judging the pool and results before expensive simulations: `sampling stats`

```sh
sampling stats designs.json
```

Read-only statistical analysis of the pool (nothing is written, the input
file is untouched): the full `doe-stats` report, one section per group of
statistics. The sections run in order — input definitions, sampling
points, evaluated results — and the first section that cannot run ends
the run with a validation error (exit 1). The report has two tiers: the
default is compact; `--verbose` adds the per-parameter detail and the
complete pair lists.

The report is also available through the dedicated `doe-stats` binary
(`crates/sensitivity/bin`) with the same sections as subcommands —
`doe-stats input`, `doe-stats sampling`, `doe-stats output` — for
pools that are not ready for the full report.

The input section prints the parameter definitions (distribution,
range, mean/sigma for gaussian, and the scope with its share of the
range; full-range scopes are not restated). It works on a fresh pool:
`init` followed by `stats` on a pool without designs and results fails
at the sampling section, `doe-stats input` shows the definitions alone.
For Gaussian parameters the section tracks the untruncated mass outside
the sampling interval: the compact tier warns when more than 10% lies
outside the scope, `--verbose` prints the below/above values against
the range and the scope (the JSON carries the facts in both tiers).
A nonpositive `sigma` is a validation error.

The sampling section reports the statistics of the sampling points
themselves (the `designs`, independent of any evaluation — this is what
to look at right after `plhs` generated
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
   all parameter pairs — the correlation criterion `plhs` optimizes
   (each pair floored at the null τ during selection) and reports as
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
coverage (how many of the pool's designs are evaluated), the
descriptive statistics n, min, max, mean, median, sample std, and the
designs holding the minimum and maximum (`min at`, `max at`). Below it
one line names the strongest input→output relations: the maximum
absolute Pearson and Spearman correlation and the maximum eta2 over
the parameters, each with the responsible parameter (`n/a` per metric
when undefined; ties name the lexicographically first parameter).
`--verbose` adds, per parameter:

1. Pearson and Spearman correlation between the parameter value and the
   result (n/a below 3 evaluated designs or without variance —
   Spearman catches monotonic-but-nonlinear trends), plus the
   correlation ratio eta2: the between-decile share of the result's
   variance, which catches non-monotonic (e.g. U-shaped) relations
   that both correlations read as ~0 (n/a below 3 evaluated designs,
   with fewer than 2 occupied deciles, or without result variance).
2. The result mean/std inside the current scope vs outside it (printed
   only when evaluated designs exist outside the scope).
3. The result mean per decile (10 equal-width bins) of the definition
   range.

The output section also correlates the result variables with each
other: every pair of result variables gets a Pearson and Spearman
correlation over the designs evaluated in both (n/a below 3 shared
designs or without variance). The compact tier prints one summary line
(the number of pairs and the maximum absolute correlations); `--verbose`
lists every pair. With fewer than two result variables the pairs block
is omitted.

A pool without `results` is a validation error (exit 1): the full
report stops at the output section and tells you so. Use
`doe-stats sampling` (or `doe-stats input`) on such a pool to see the
sections that do run. The tool reports statistics; the judgment — run
expensive simulations or not, where to set the scope for `--add` —
stays with you. Exit codes as above; a pool with no designs at all, or
a result name that matches no design, is a validation error (exit 1).

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