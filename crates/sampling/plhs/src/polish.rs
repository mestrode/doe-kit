//! Deterministic post-placement refinement (swap descent).
//!
//! The batched selection of [`crate::augment`] places its designs chunk
//! by chunk; every chunk is optimized on its own — exact within the
//! chunk, greedy above the cap — which leaves the finished assignment
//! improvable: greedy chunks drift, and even a run of exact chunks is
//! only per-chunk optimal, never jointly. This pass is a deterministic
//! greedy descent over the finished assignment: a move swaps the values
//! of two new designs within one parameter, which keeps every
//! per-parameter value multiset (and therefore the Latin Hypercube
//! property) structurally intact. A swap in parameter `j` changes only
//! the pairs involving `j`; each is scored in O(1) from raw running
//! sums. Sweeps scan all candidate swaps in lexicographic order and
//! apply strictly improving ones immediately; the pass stops at the
//! first sweep without an improvement (convergence) or when the
//! evaluation budget is exhausted. No randomness anywhere.

use crate::scoring::{PairSeries, assignment_score};

/// Minimum score decrease for a swap to count as an improvement; deltas
/// below this are float noise, not structure.
const IMPROVE_EPS: f64 = 1e-12;

/// Total budget of pair evaluations across all sweeps; one full sweep
/// costs `sweep_cost` (see [`sweep_budget`]) evaluations, so the budget
/// caps the number of sweeps at 32 while guaranteeing at least one full
/// sweep on every instance.
const EVAL_BUDGET: u128 = 200_000_000;

/// Upper bound on sweeps regardless of budget.
const MAX_SWEEPS: u128 = 32;

/// Result of one polish pass.
#[derive(Debug, Clone, PartialEq)]
pub struct PolishReport {
    /// Number of swaps applied.
    pub moves: usize,
    /// Score before the pass (reference scorer, over base + all new
    /// designs).
    pub score_before: f64,
    /// Score after the pass (same scorer).
    pub score_after: f64,
}

/// Which side of a pair a swapped parameter is.
#[derive(Debug, Clone, Copy)]
enum Side {
    /// The parameter is `pair.a`: its values form the x series.
    X,
    /// The parameter is `pair.b`: its values form the y series.
    Y,
}

/// Which side of the pair the parameter `j` is, for every pair
/// involving `j` (`(pair index, side)`, ascending in pair index).
fn affected_sides(pairs: &[PairSeries], j: usize) -> Vec<(usize, Side)> {
    pairs
        .iter()
        .enumerate()
        .filter_map(|(k, pair)| {
            if pair.a == j {
                Some((k, Side::X))
            } else if pair.b == j {
                Some((k, Side::Y))
            } else {
                None
            }
        })
        .collect()
}

/// Evaluations of one full sweep: every candidate swap `(j, a, b)`
/// evaluates `affected[j]` pairs.
fn sweep_cost(affected: &[Vec<(usize, Side)>], m: usize) -> u128 {
    if m < 2 {
        return 0;
    }
    let candidates = m * (m - 1) / 2;
    affected.iter().map(|a| a.len() as u128).sum::<u128>()
        * u128::try_from(candidates).unwrap_or(u128::MAX)
}

/// Number of sweeps the evaluation budget pays for: between 1 and
/// [`MAX_SWEEPS`] full sweeps, `0` when a sweep costs nothing (there is
/// nothing to evaluate — no pairs or fewer than two designs).
fn sweep_budget(sweep_cost: u128) -> u128 {
    if sweep_cost == 0 {
        0
    } else {
        EVAL_BUDGET
            .checked_div(sweep_cost)
            .unwrap_or(u128::MAX)
            .clamp(1, MAX_SWEEPS)
    }
}

/// Running raw sums of one pair's series — the base values plus all
/// placed designs — supporting O(1) two-sample swaps: a swap in one
/// parameter changes two samples of every pair involving it, and raw
/// sums update exactly.
#[derive(Debug, Clone)]
struct PairTotals {
    len: usize,
    sx: f64,
    sy: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

impl PairTotals {
    /// Sums of a base series.
    fn new(xs: &[f64], ys: &[f64]) -> Self {
        let mut totals = PairTotals {
            len: 0,
            sx: 0.0,
            sy: 0.0,
            sxx: 0.0,
            syy: 0.0,
            sxy: 0.0,
        };
        for (&x, &y) in xs.iter().zip(ys.iter()) {
            totals.push(x, y);
        }
        totals
    }

    /// Folds one more sample into the running sums.
    fn push(&mut self, x: f64, y: f64) {
        self.len += 1;
        self.sx += x;
        self.sy += y;
        self.sxx += x * x;
        self.syy += y * y;
        self.sxy += x * y;
    }

    /// Squared Pearson correlation of the accumulated series from the
    /// raw sums, mirroring the conventions of
    /// [`crate::scoring::pearson`]: `0.0` for fewer than two samples
    /// and for zero variance on either side; the coefficient is
    /// clamped before squaring.
    fn r2(&self) -> f64 {
        Self::r2_of(self.len, self.sx, self.sy, self.sxx, self.syy, self.sxy)
    }

    /// Shared raw-sums r^2 with the conventions of
    /// [`crate::scoring::pearson`].
    fn r2_of(len: usize, sx: f64, sy: f64, sxx: f64, syy: f64, sxy: f64) -> f64 {
        if len < 2 {
            return 0.0;
        }
        let nf = len as f64;
        let cxx = sxx - sx * sx / nf;
        let cyy = syy - sy * sy / nf;
        let cxy = sxy - sx * sy / nf;
        if cxx <= 0.0 || cyy <= 0.0 {
            return 0.0;
        }
        let r = (cxy / (cxx * cyy).sqrt()).clamp(-1.0, 1.0);
        r * r
    }

    /// [`PairTotals::r2`](Self::r2) if the samples `(xa, ya)` and
    /// `(xb, yb)` were re-paired as `(xb, ya)` and `(xa, yb)` — exactly
    /// what a within-parameter value swap does to the pair. Only the
    /// cross term `sxy` changes: every marginal sum (`sx`, `sy`, `sxx`,
    /// `syy`) is a sum over a fixed multiset, which is the Latin
    /// Hypercube property in arithmetic form.
    fn r2_with_swap(&self, xa: f64, xb: f64, ya: f64, yb: f64) -> f64 {
        Self::r2_of(
            self.len,
            self.sx,
            self.sy,
            self.sxx,
            self.syy,
            self.sxy - xa * ya - xb * yb + xb * ya + xa * yb,
        )
    }

    /// Applies a re-pairing swap to the running sums (see
    /// [`PairTotals::r2_with_swap`]).
    fn apply_swap(&mut self, xa: f64, xb: f64, ya: f64, yb: f64) {
        self.sxy += xb * ya + xa * yb - xa * ya - xb * yb;
    }
}

/// Raw sums of every pair over its base series plus all placed values.
fn pair_totals(pairs: &[PairSeries], placed_t: &[Vec<f64>], m: usize) -> Vec<PairTotals> {
    pairs
        .iter()
        .map(|pair| {
            let mut totals = PairTotals::new(&pair.xs, &pair.ys);
            let xs = placed_t[pair.a].iter().take(m);
            let ys = placed_t[pair.b].iter().take(m);
            for (&x, &y) in xs.zip(ys) {
                totals.push(x, y);
            }
            totals
        })
        .collect()
}

/// Runs the descent over the placed values of the `m` new designs.
///
/// `placed_t[j][i]` is the scope-transformed value of parameter `j` and
/// new design `i`; `mirror` holds opaque parallel values (the real
/// output values) that are swapped in lockstep, so the caller's output
/// stays consistent with the refined assignment without this module
/// knowing anything about real-space transforms.
pub(crate) fn polish(
    pairs: &[PairSeries],
    placed_t: &mut [Vec<f64>],
    mirror: &mut [Vec<f64>],
) -> PolishReport {
    let p = placed_t.len();
    let m = placed_t.first().map_or(0, |v| v.len());
    let score_before = assignment_score(pairs, placed_t, m);

    // Pairs affected by a swap in parameter j, and the sweep cost and
    // budget they imply.
    let affected: Vec<Vec<(usize, Side)>> = (0..p).map(|j| affected_sides(pairs, j)).collect();
    let cost = sweep_cost(&affected, m);
    let budget = sweep_budget(cost).saturating_mul(cost);

    let mut evals: u128 = 0;
    let mut moves = 0usize;
    'sweeps: loop {
        // rebuild every sweep: in-place sum updates accumulate rounding
        // over thousands of applied swaps, and a per-sweep rebuild is
        // negligible next to the sweep itself
        let mut totals = pair_totals(pairs, placed_t, m);
        let mut applied = 0usize;
        for j in 0..p {
            for a in 0..m {
                for b in (a + 1)..m {
                    if evals >= budget {
                        break 'sweeps;
                    }
                    evals += affected[j].len() as u128;
                    // delta over the pairs involving j; pairs without j
                    // cannot change
                    let (ja, jb) = (placed_t[j][a], placed_t[j][b]);
                    let mut delta = 0.0;
                    for &(k, side) in &affected[j] {
                        let pair = &pairs[k];
                        // the partner parameter of the pair: its values
                        // at the two positions stay fixed while j's
                        // values swap
                        let (oa, ob) = match side {
                            Side::X => (placed_t[pair.b][a], placed_t[pair.b][b]),
                            Side::Y => (placed_t[pair.a][a], placed_t[pair.a][b]),
                        };
                        delta += totals[k].r2_with_swap(ja, jb, oa, ob) - totals[k].r2();
                    }
                    if delta < -IMPROVE_EPS {
                        placed_t[j].swap(a, b);
                        mirror[j].swap(a, b);
                        for &(k, side) in &affected[j] {
                            let pair = &pairs[k];
                            let (oa, ob) = match side {
                                Side::X => (placed_t[pair.b][a], placed_t[pair.b][b]),
                                Side::Y => (placed_t[pair.a][a], placed_t[pair.a][b]),
                            };
                            totals[k].apply_swap(ja, jb, oa, ob);
                        }
                        moves += 1;
                        applied += 1;
                    }
                }
            }
        }
        if applied == 0 {
            break; // converged: a full sweep without an improvement
        }
    }

    let score_after = assignment_score(pairs, placed_t, m);
    PolishReport {
        moves,
        score_before,
        score_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::pearson;

    fn pair(a: usize, b: usize, xs: Vec<f64>, ys: Vec<f64>) -> PairSeries {
        PairSeries { a, b, xs, ys }
    }

    fn sorted(values: &[f64]) -> Vec<f64> {
        let mut v = values.to_vec();
        v.sort_by(|x, y| x.partial_cmp(y).unwrap());
        v
    }

    #[test]
    fn polish_improves_a_correlated_assignment_and_keeps_the_permutations() {
        // two parameters on a fresh pool (no base samples), six new
        // designs, identical value multisets in the same order: a
        // perfectly correlated placement — the greedy drift pattern
        let xs: Vec<f64> = [0.05, 0.25, 0.35, 0.55, 0.75, 0.95].to_vec();
        let pairs = vec![pair(0, 1, Vec::new(), Vec::new())];
        let mut t = vec![xs.clone(), xs.clone()];
        // the mirror carries scaled copies: it must follow every swap
        let mut mirror: Vec<Vec<f64>> = t
            .iter()
            .map(|v| v.iter().map(|x| x * 10.0).collect())
            .collect();
        let original_sorted: Vec<Vec<f64>> = t.iter().map(|v| sorted(v)).collect();

        let report = polish(&pairs, &mut t, &mut mirror);

        // the assignment was perfectly correlated: r^2 = 1 exactly
        assert!(
            (report.score_before - 1.0).abs() < 1e-9,
            "score_before: {}",
            report.score_before
        );
        assert!(report.moves > 0, "a drift assignment must have moves");
        assert!(
            report.score_after < report.score_before - 1e-9,
            "score must strictly improve: before {} after {}",
            report.score_before,
            report.score_after
        );
        // Latin Hypercube property: every per-parameter multiset is
        // preserved (a swap only exchanges values between designs)
        for j in 0..2 {
            assert_eq!(
                sorted(&t[j]),
                original_sorted[j],
                "parameter {j} multiset changed"
            );
        }
        // the mirror is swapped in lockstep: still exactly 10x
        for j in 0..2 {
            for i in 0..t[j].len() {
                assert!(
                    (mirror[j][i] - t[j][i] * 10.0).abs() < 1e-12,
                    "mirror out of sync at ({j}, {i})"
                );
            }
        }
        // the reported scores match an independent recompute
        let recomputed = assignment_score(&pairs, &t, t[0].len());
        assert!(
            (report.score_after - recomputed).abs() < 1e-9,
            "score_after {} vs recomputed {recomputed}",
            report.score_after
        );
    }

    #[test]
    fn polish_keeps_an_assignment_without_improving_swaps() {
        // two designs, one pair: the only candidate swap (x of design 0
        // and 1) flips the correlation sign, r^2 is unchanged — the
        // descent must converge with zero moves and equal scores
        let pairs = vec![pair(0, 1, Vec::new(), Vec::new())];
        let mut t = vec![vec![0.25, 0.75], vec![0.25, 0.75]];
        let mut mirror = vec![vec![2.5, 7.5], vec![2.5, 7.5]];

        let report = polish(&pairs, &mut t, &mut mirror);
        assert_eq!(report.moves, 0);
        assert!(
            (report.score_after - report.score_before).abs() < 1e-12,
            "scores changed without moves"
        );
        assert!((report.score_after - 1.0).abs() < 1e-9);
        // the assignment itself is untouched
        assert_eq!(t, vec![vec![0.25, 0.75], vec![0.25, 0.75]]);
        assert_eq!(mirror, vec![vec![2.5, 7.5], vec![2.5, 7.5]]);
    }

    #[test]
    fn substituted_r2_matches_a_full_recompute() {
        // a pair with a base series plus six placed samples; the O(1)
        // substitution scorers must track a full two-pass recompute
        let xs = vec![0.1, 0.5, 0.9];
        let ys = vec![0.9, 0.6, 0.1];
        let pairs = [pair(0, 1, xs, ys)];
        let t = [
            vec![0.15, 0.45, 0.65, 0.25, 0.85, 0.35],
            vec![0.55, 0.25, 0.05, 0.95, 0.45, 0.75],
        ];
        let mut totals = PairTotals::new(&pairs[0].xs, &pairs[0].ys);
        for (&x, &y) in t[0].iter().zip(&t[1]) {
            totals.push(x, y);
        }

        // current r^2
        let mut full_x = pairs[0].xs.clone();
        let mut full_y = pairs[0].ys.clone();
        full_x.extend_from_slice(&t[0]);
        full_y.extend_from_slice(&t[1]);
        let expected = pearson(&full_x, &full_y).powi(2);
        assert!(
            (totals.r2() - expected).abs() < 1e-9,
            "r2 {} vs reference {expected}",
            totals.r2()
        );

        // x swap at placed positions 1 and 4 (the base series has 3
        // samples, so the placed values live at indices 3..9)
        let (xa, xb) = (t[0][1], t[0][4]);
        let (ya, yb) = (t[1][1], t[1][4]);
        let substituted = totals.r2_with_swap(xa, xb, ya, yb);
        full_x[4] = xb;
        full_x[7] = xa;
        let expected_x = pearson(&full_x, &full_y).powi(2);
        assert!(
            (substituted - expected_x).abs() < 1e-9,
            "x-substitution {substituted} vs reference {expected_x}"
        );

        // y swap at placed positions 0 and 3 (x values at those
        // positions stay; restore the x swap of the previous section
        // first)
        full_x[4] = xa;
        full_x[7] = xb;
        let (ya0, yb0) = (t[1][0], t[1][3]);
        let (xa0, xb0) = (t[0][0], t[0][3]);
        let substituted_y = totals.r2_with_swap(ya0, yb0, xa0, xb0);
        let mut full_y2 = full_y.clone();
        full_y2[3] = yb0;
        full_y2[6] = ya0;
        let expected_y = pearson(&full_x, &full_y2).powi(2);
        assert!(
            (substituted_y - expected_y).abs() < 1e-9,
            "y-substitution {substituted_y} vs reference {expected_y}"
        );

        // applying a swap makes the running sums agree with the
        // substitution preview
        let mut applied = totals.clone();
        applied.apply_swap(xa, xb, ya, yb);
        assert!(
            (applied.r2() - substituted).abs() < 1e-12,
            "applied {} vs preview {substituted}",
            applied.r2()
        );
        let mut applied_y = totals.clone();
        applied_y.apply_swap(ya0, yb0, xa0, xb0);
        assert!(
            (applied_y.r2() - substituted_y).abs() < 1e-12,
            "applied {} vs preview {substituted_y}",
            applied_y.r2()
        );
    }

    #[test]
    fn sweep_budget_is_bounded_and_scales_with_cost() {
        // a cheap sweep buys the full sweep cap
        assert_eq!(sweep_budget(1), MAX_SWEEPS);
        // a sweep at the budget buys exactly one
        assert_eq!(sweep_budget(EVAL_BUDGET), 1);
        assert_eq!(sweep_budget(EVAL_BUDGET * 10), 1);
        // in between, the budget divides by the sweep cost
        assert_eq!(sweep_budget(EVAL_BUDGET / 100), 100.min(MAX_SWEEPS));
        // nothing to evaluate: no budget at all
        assert_eq!(sweep_budget(0), 0);
    }

    #[test]
    fn polish_is_deterministic() {
        let xs: Vec<f64> = [0.05, 0.25, 0.35, 0.55, 0.75, 0.95].to_vec();
        let pairs = vec![pair(0, 1, vec![0.2, 0.7], vec![0.6, 0.3])];
        let build = || (vec![xs.clone(), xs.clone()], vec![xs.clone(), xs.clone()]);

        let (mut t1, mut m1) = build();
        let (mut t2, mut m2) = build();
        let r1 = polish(&pairs, &mut t1, &mut m1);
        let r2 = polish(&pairs, &mut t2, &mut m2);
        assert_eq!(r1, r2);
        assert_eq!(t1, t2);
        assert_eq!(m1, m2);
    }
}
