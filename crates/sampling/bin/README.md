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
| `init` | create a new pool file with two example parameters |
| `plhs` | pooled Latin Hypercube Sampling (the `plhs` crate) |
| `stats` | statistical analysis of the pool: sampling points and evaluated results (the `doe-stats` crate) |

## Creating a new pool: `sampling init`

```sh
sampling init designs.json
```

Writes a skeleton pool with two example parameters (`param_uniform`,
`param_gaussian`) and an empty `designs` list — edit `params` by hand, then
run `plhs` or `stats` on the file. The skeleton sets no `scope` (the full
parameter ranges apply) and no `results` (nothing is evaluated yet), so it
is immediately valid. `init` refuses to overwrite an existing file
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
`n_params`, `strategy` (`exact`/`greedy`, the selection strategy the run
actually used), `score` and `elapsed_ms` (the compute phase only: after
reading the pool, before writing it). The `stats` object carries
`subcommand`, `file`, `evaluated`, `pool` and `results`, mirroring the
human report's numbers. This flag is the output convention for every
future sampling-method subcommand, so benchmark runs can be collected
line by line into one file.

## Judging the pool and results before expensive simulations: `sampling stats`

```sh
sampling stats designs.json
```

Read-only statistical analysis of the pool (nothing is written, the input
file is untouched). Two blocks are reported:

Statistics of the sampling points themselves (the `designs`, independent
of any evaluation — this is what to look at right after `plhs` generated
new designs). The pool line opens the report with the total, the
evaluated count and the context number of designs per parameter:

1. Per parameter: descriptive statistics of the design values (n, min,
   max, mean, median, sample std).
2. Per parameter: sampling-point counts per decile (10 equal-width bins)
   of the definition range — how uniformly the range is covered — plus
   the outer deciles (points in the first + last decile), telling
   whether the range edges are populated.
3. Space-filling metrics over all designs in the unit cube (every
   parameter linearly scaled to [0, 1] of its definition range, so
   mixed units stay comparable): the minimal pairwise distance (near
   zero means near-duplicate parameter sets) and the mean
   nearest-neighbor distance (clumping); both n/a for a single design.
4. The centered L2 discrepancy (CL2, Hickernell) in the same normalized
   space: a single uniformity number for the whole pool, comparable
   between candidate pools — run `--explore` with a different `--seed`
   and compare; lower is better.
5. Per parameter pair: Pearson and Spearman correlation between the
   parameter values (n/a below 3 designs or without variance).

Statistics of the evaluated `results` (reported only when `results`
exist). Per numeric result variable (any numeric field on a result
entry, e.g. `energy`):

1. Coverage: how many of the pool's designs are evaluated.
2. Descriptive statistics: n, min, max, mean, median, sample std.
3. Per parameter: Pearson and Spearman correlation between the parameter
   value and the result (n/a below 3 evaluated designs or without
   variance — Spearman catches monotonic-but-nonlinear trends).
4. Per parameter: result mean/std inside the current scope vs outside it
   (printed only when evaluated designs exist outside the scope).
5. Per parameter: result mean per decile (10 equal-width bins) of the
   definition range.

A pool without `results` is no longer an error: the sampling points are
analyzed on their own, so `init` followed by `plhs` and `stats` reports
how well the generated points are distributed and whether they
correlate. The tool reports statistics; the judgment — run expensive
simulations or not, where to set the scope for `--add` — stays with you.
Exit codes as above; a pool with no designs at all, or a result name
that matches no design, is a validation error (exit 1).

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