# slhs

Pooled Latin Hypercube Sampling. The Rust crate is published as [`plhs` on
crates.io](https://crates.io/crates/plhs); the binary is called `slhs`.

`slhs` augments a pool of existing experiment designs with new designs that
preserve the Latin Hypercube property inside a per-parameter **scope** (a
sub-range of each parameter's definition range), while keeping the
correlations between all parameter pairs low.

## Install

```sh
cargo install plhs
```

Library use:

```toml
[dependencies]
plhs = "0.1"
```

## Usage

```sh
slhs --add 2 designs.json
```

Options:

| Option | Description |
| --- | --- |
| `--add N` (required) | number of new designs to add |
| `--seed N` | random seed, default 42 (fixed, for reproducible runs) |
| `--output FILE` | output file, default: the input file |
| `--no-backup` | skip writing the backup file |
| `--cap N` | exact-enumeration cap, default 1000000; above it, greedy sequential selection is used |

The input file is read, `N` new designs are appended to `designs`,
`new_designs` is overwritten with the new batch, and the result is written
back atomically. A backup of the original input is written to
`<input>.bak` unless `--no-backup` is given. Unknown JSON fields (including
`results`) and key order are preserved.

Exit codes: `0` success, `1` validation error, `2` I/O error.

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
parameter** (the Latin Hypercube property) with the lowest score is chosen.
The score of an assignment is the sum of squared Pearson correlations over
all parameter pairs, computed on the transformed values of the shared
in-scope designs plus all new candidates. Assignments are enumerated
exactly up to `--cap`; beyond that, a deterministic greedy fallback places
designs one at a time. All computation happens in `[0, 1]` space; real
values are written only in the output.

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
- `designs`: all designs (old + new, appended).
- `new_designs`: the designs added by the last run (overwritten).
- `results`: output values per design name; passed through untouched.

New designs are named `{iteration}.{design}` with `design` starting at 1;
the next iteration is detected from the existing names (maximum found plus
one, 1 when no name matches the pattern).

## Determinism

Candidate values are drawn from a ChaCha8 RNG seeded with `--seed` (default
42). The same input file and seed always produce the same output, verified
by a byte-for-byte golden test in CI.

## Development

All changes go through pull requests; direct commits to `main` are not
allowed. CI runs `cargo fmt --check`, `cargo clippy -- -D warnings` and
`cargo test` on every pull request.

## License

MIT
