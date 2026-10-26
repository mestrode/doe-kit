# doe-simio

`simio`: simulation input/output for the doe-kit design pool. Simulation
results enter the pool as `results` entries — one object per evaluated
design, its `name` matching the design, every numeric field a result
variable (see the `doe-pool` README for the pool format).

## Subcommands

| Subcommand | Purpose |
| --- | --- |
| `test` | evaluate the analytic example functions on every unevaluated design and write the results into the pool |

Planned: `export` (designs into per-design workspace folders for
external simulators), `exec` (run an external simulation binary in the
workspaces), `import` (external results back into the pool). They are
deliberately absent until their implementations land.

## Usage

```sh
simio test designs.json
```

Evaluates the deterministic analytic example functions on every design
of the pool that has no `results` entry yet; already-solved designs are
skipped and their entries stay untouched. When nothing is left to
evaluate, the file is not written at all. The parameters `a`, `b`, `c`
(the `sampling init` example parameters) are resolved by name; extra
parameters are allowed and ignored. Missing ones are a validation error
(exit 1).

Options: `--json` (one machine-readable line on stdout instead of the
human summary; errors stay human on stderr), `--no-backup` (skip the
`.bak` backup of the input). Exit codes: 0 success, 1 validation error,
2 I/O error.

## Example functions

Graded by difficulty, deterministic, no seed — a testbed for the whole
workflow (init → explore → test → stats) and later for the metamodel
crates, which can be validated against the known ground truth:

- `linear` = 0.7854·a + 2.5·b + 0.5·c — pure first-order baseline; any
  surrogate that fails this is broken.
- `bowl` = (a−5)² + b² + (c−2.5)² + 0.3·a·b — convex quadratic bowl,
  zero at the pool's center point; the interaction term shifts the
  true minimum slightly below it.
- `wave` = 10·sin(0.6283·a)·exp(−(b+1)/2) + 0.5·sin(1.2566·c) — damped
  oscillation plus a second oscillation source; non-polynomial, the
  hard case for polynomial surrogates.
