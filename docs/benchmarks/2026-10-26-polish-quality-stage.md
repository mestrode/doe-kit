# plhs Quality-Stage Benchmark (post-placement polish)

Date: 2026-10-26
Tool: `sampling plhs --explore`, release profile, `--cap 1000000`
Scope: quality stage from PR #25 / issue #24 (deterministic post-placement
swap descent, always on). All instances run on fresh pools of uniform
parameters (`params{2,8,15,50}uni.json`), 3 seeds each (0, 42, 156).
Scores are the pooled sum of squared Pearson r over all parameter pairs
(lower is better). "before"/"after" are `polish_score_before` /
`polish_score_after` from the `--json` output.

## Results (median over 3 seeds)

| Instance | Strategy | Chunks | Score before | Score after | Reduction | Median moves | Median elapsed |
|---|---|---|---|---|---|---|---|
| p=2, m=10 | greedy | 1 | 1.92e-01 | 4.82e-06 | ~4.0e4x | 10 | 0 ms |
| p=2, m=50 | greedy | 1 | 4.25e-01 | 7.48e-11 | ~5.7e9x | 200 | 0 ms |
| p=2, m=1000 | greedy | 1 | 4.02e-01 | 1.26e-13 | ~3.2e12x | 82,215 | 2.1 s |
| p=8, m=10 | greedy | 2 | 2.99e+00 | 3.83e-02 | ~78x | 71 | 1.2 s |
| p=8, m=30 | mixed | 5 | 1.76e-01 | 1.76e-04 | ~1.0e3x | 169 | 13.5 s |
| p=8, m=50 | mixed | 8 | 9.58e-02 | 7.34e-06 | ~1.3e4x | 292 | 39.1 s |
| p=15, m=10 | exact | 5 | 8.52e+00 | 5.05e+00 | ~1.7x | 107 | 214 ms |
| p=15, m=50 | exact | 25 | 6.26e-01 | 1.35e-04 | ~4.6e3x | 860 | 2.5 s |
| p=15, m=200 | exact | 100 | 4.05e-02 | 1.31e-07 | ~3.1e5x | 1,574 | 46.0 s |
| p=50, m=10 | exact | 10 | 1.39e+02 | 1.14e+02 | ~1.2x | 291 | 4 ms |
| p=50, m=50 | exact | 50 | 2.51e+01 | 5.42e-01 | ~46x | 9,468 | 340 ms |
| p=50, m=1000 | exact | 1000 | 1.24e+00 | 3.99e-08 | ~3.1e7x | 35,700 | 6.0 s |

Single-chunk exact runs are skipped by design (provable no-op); every run in
this matrix is multi-chunk or greedy/mixed, so all of them were polished.

## Findings

1. **Polish closes the deficit paths identified in issue #24.** The
   p=2/m=1000 greedy drift (single-chunk trajectory, exact unreachable at
   (1000!)^2 combinations) lands at ~1e-13 instead of ~0.4 — a factor of
   ~3e12, effectively exact-quality where exact can never run. The
   p=50/m=50 forced-b=1 case drops from ~25 to ~0.54.
2. **Every instance improves monotonically; seeds barely matter.** No seed
   made any instance worse. Post-polish scores across seeds 0/42/156 differ
   by at most one order of magnitude per instance; the ranking of instances
   is identical for all seeds.
3. **The remaining absolute deficits are small-m, high-p instances.**
   Normalized per pair: p=50/m=10 sits at ~0.093 r2 per pair (113.9 over
   1225 pairs), p=15/m=10 at ~0.048 (5.05 over 105 pairs), while all other
   instances land below ~0.001 per pair. With only m=10 rows there are very
   few swap candidates, and the descent converges in a shallow local
   minimum. Increasing m (or running explore in several passes) is the
   practical remedy; this matches the known "designs per parameter >= 10
   for screening" guidance.
4. **Batched-exact instances benefit too** (p=15, p=50 at m>=50): per-chunk
   optimality is not joint optimality, and the polish pass collects
   cross-chunk gains of 46x to 3e5x.
5. **Polish is not the runtime bottleneck.** Elapsed is dominated by
   placement: the slowest instances (p=8 m=30/50, p=15 m=200 at 13-46 s)
   spend their time in chunk selection, not in the swap descent. The
   largest polish effort (p=2/m=1000, up to 143,267 moves) still completes
   within the ~2 s total of that run.

## Verdict

The quality stage delivers on the issue #24 goal: greedy, mixed and forced
selection paths reach near-exact pooled-correlation scores, including the
instances exact enumeration can never reach. Quality at p>=15/m=10 remains
limited by the small number of rows, not by the optimizer.

Raw `--json` lines: 12 unique instances x 3 seeds (36 runs total),
collected via `>> results.jsonl`.
