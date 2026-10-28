//! Deterministic post-placement refinement (swap descent), issue #46:
//! a two-phase greedy descent under the combined objective.
//!
//! The batched selection of [`crate::augment`] places its designs chunk
//! by chunk; every chunk is optimized on its own — exact within the
//! chunk, greedy above the cap — which leaves the finished assignment
//! improvable: greedy chunks drift, and even a run of exact chunks is
//! only per-chunk optimal, never jointly.
//!
//! **Phase 1 (r² descent)** is a greedy descent over the pooled pair
//! r², the score the selection optimizes. A move swaps the values of
//! two new designs within one parameter, which keeps every
//! per-parameter value multiset (and therefore the Latin Hypercube
//! property) structurally intact. A swap in parameter `j` changes only
//! the pairs involving `j`; each is scored in O(1) from raw running
//! sums. Sweeps scan all candidate swaps in lexicographic order and
//! apply strictly improving ones immediately.
//!
//! **Phase 2 (space-filling descent)** fixes what r² cannot see: once
//! the pooled r² sits in its ≈ 0 basin the objective is flat — an
//! X-shaped assignment (two crossing lines) scores identically to a
//! genuinely spread one on every pair, because zero linear correlation
//! is not independence. Phase 2 therefore descends the Audze–Eglais
//! potential energy ([`crate::space::ae_energy`]) of the active point
//! set — the base designs in scope for every parameter plus the new
//! designs. Its guard is the τ-floor ladder of
//! [`crate::space::TAU_TIERS`], tightest tier first: a swap is admitted
//! only if every pair it touches lands at or below the active tier's
//! per-pair r² floor. A tolerance tighter than the `O(1/m)` r² steps
//! of the swap landscape would freeze the search inside the
//! null-correlation manifold — empirically the initially considered
//! ε = 1e-9 admits no move at all from an X at m = 8 and only
//! microscopic creep at m = 100 — so the ladder instead spends real
//! but practically null r² headroom (|r| ≤ 0.1 at the loosest tier,
//! far inside the project's |r| < 0.3 acceptance rule) to buy genuine
//! space-filling. A swap in parameter `j` changes only the distances
//! of the two swapped designs to every other point, each recomputed
//! in O(1) from a distance matrix. Phase 2 runs its own evaluation
//! budget counted in distance terms, shared across the ladder; each
//! tier is additionally capped at [`MAX_SWEEPS`] sweeps, and the
//! budget is a hard cap — an instance whose single sweep alone
//! exceeds it scans a deterministic lexicographic prefix instead of
//! forcing the full sweep through.
//!
//! The pass stops phase 1 at the first sweep without an improvement
//! (convergence) or when its budget is exhausted, and phase 2 at each
//! tier's convergence, the tier sweep cap, or the shared phase-2
//! budget. No randomness anywhere.

use crate::scoring::{PairSeries, assignment_score};
use crate::space::{TAU_TIERS, ae_energy, ae_term};

/// Minimum decrease for a swap to count as an improvement; deltas below
/// this are float noise, not structure. Phase 1 compares pooled r²
/// deltas; phase 2 compares raw AE term-sum deltas.
const IMPROVE_EPS: f64 = 1e-12;

/// Total budget of pair evaluations across all phase-1 sweeps; one full
/// sweep costs `sweep_cost` (see [`sweep_budget`]) evaluations, so the
/// budget caps the number of sweeps at 32 while guaranteeing at least
/// one full sweep on every instance. The same constant bounds the
/// phase-2 distance-term budget, shared across the ladder tiers.
const EVAL_BUDGET: u128 = 200_000_000;

/// Upper bound on sweeps regardless of budget: total for phase 1, and
/// per ladder tier for phase 2 (the tiers share the phase-2 budget).
const MAX_SWEEPS: u128 = 32;

/// Result of one polish pass.
#[derive(Debug, Clone, PartialEq)]
pub struct PolishReport {
    /// Swaps applied by phase 1 (r² descent).
    pub moves: usize,
    /// Swaps applied by phase 2 (space-filling descent).
    pub ae_moves: usize,
    /// Pooled pair r² before the pass (reference scorer, over base +
    /// all new designs).
    pub score_before: f64,
    /// Pooled pair r² after the pass (same scorer).
    pub score_after: f64,
    /// AE energy after phase 1 — the phase-2 starting point (see
    /// [`crate::space::ae_energy`]).
    pub ae_before: f64,
    /// AE energy after phase 2.
    pub ae_after: f64,
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

/// Evaluations of one full phase-1 sweep: every candidate swap
/// `(j, a, b)` evaluates `affected[j]` pairs.
fn sweep_cost(affected: &[Vec<(usize, Side)>], m: usize) -> u128 {
    if m < 2 {
        return 0;
    }
    let candidates = m * (m - 1) / 2;
    affected.iter().map(|a| a.len() as u128).sum::<u128>()
        * u128::try_from(candidates).unwrap_or(u128::MAX)
}

/// Number of phase-1 sweeps the evaluation budget pays for: between 1
/// and [`MAX_SWEEPS`] full sweeps, `0` when a sweep costs nothing (there
/// is nothing to evaluate — no pairs or fewer than two designs).
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

/// Distance-term evaluations of one full phase-2 sweep: every candidate
/// swap `(j, a, b)` evaluates the AE terms of the two swapped designs
/// against every other point of the active set — `2 * (n - 2)` terms —
/// so the sweep costs `p * C(m, 2) * 2 * (n - 2)`. `0` when no swap
/// can change the energy: fewer than two parameters, fewer than two
/// movable designs, or an active set of two points whose only pair is
/// swap-invariant.
fn phase2_sweep_cost(p: usize, m: usize, n: usize) -> u128 {
    if p < 2 || m < 2 || n < 3 {
        return 0;
    }
    let candidates = (p as u128) * ((m as u128) * (m as u128 - 1)) / 2;
    candidates * 2 * (n as u128 - 2)
}

/// Phase-2 evaluation budget in distance-term evaluations, shared
/// across all tiers of the ladder: a HARD cap — deliberately deviating
/// from phase 1's minimum-sweep rule, a single sweep that alone
/// exceeds it is not forced through (one O(m² · n) sweep on a large
/// instance would burn the budget's purpose as a runtime bound). Such
/// an instance scans a deterministic lexicographic prefix of its
/// first sweep instead; every instance the budget can pay for runs
/// whole sweeps, with each tier additionally capped at [`MAX_SWEEPS`]
/// sweeps.
fn phase2_budget(cost: u128) -> u128 {
    if cost == 0 { 0 } else { EVAL_BUDGET }
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

/// The active point set of the space-filling criterion as rows: the
/// base points (designs in scope for every parameter, already
/// transformed to the unit cube) followed by the `m` new designs,
/// built from the per-parameter columns `placed_t`.
pub(crate) fn active_rows(base_points: &[Vec<f64>], placed_t: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let p = placed_t.len();
    let m = placed_t.first().map_or(0, |v| v.len());
    let mut rows = base_points.to_vec();
    rows.extend((0..m).map(|i| (0..p).map(|j| placed_t[j][i]).collect()));
    rows
}

/// Symmetric matrix of squared distances over the active point set.
fn distance_matrix(base_points: &[Vec<f64>], placed_t: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let rows = active_rows(base_points, placed_t);
    let n = rows.len();
    let mut d2 = vec![vec![0.0; n]; n];
    for i in 0..n {
        for k in (i + 1)..n {
            let mut s = 0.0;
            for (&x, &y) in rows[i].iter().zip(rows[k].iter()) {
                let d = x - y;
                s += d * d;
            }
            d2[i][k] = s;
            d2[k][i] = s;
        }
    }
    d2
}

/// Raw AE term-sum delta of swapping the coordinate-`j` values of the
/// active-set points `ia` and `ib`, given the current squared-distance
/// matrix and the coordinate-`j` values of all points. Only the
/// distances of the two swapped points to every other point change;
/// the distance between the two swapped points is invariant. The
/// delta is the unnormalized term sum; acceptance compares it against
/// [`IMPROVE_EPS`] directly.
fn ae_swap_delta(d2: &[Vec<f64>], coord_j: &[f64], ia: usize, ib: usize) -> f64 {
    let (ja, jb) = (coord_j[ia], coord_j[ib]);
    let mut delta = 0.0;
    for k in 0..coord_j.len() {
        if k == ia || k == ib {
            continue;
        }
        let xk = coord_j[k];
        // squared coordinate contributions of the current values ...
        let (ma, mb) = ((ja - xk) * (ja - xk), (jb - xk) * (jb - xk));
        // ... and the swapped distances they imply
        delta += ae_term(d2[ia][k] - ma + mb) - ae_term(d2[ia][k]);
        delta += ae_term(d2[ib][k] - mb + ma) - ae_term(d2[ib][k]);
    }
    delta
}

/// Applies a coordinate-`j` swap between the active-set points `ia`
/// and `ib` to the squared-distance matrix: the rows (and columns) of
/// the two points are rewritten from the old coordinate values `ja`
/// and `jb`.
fn apply_ae_swap(d2: &mut [Vec<f64>], coord_j: &[f64], ia: usize, ib: usize, ja: f64, jb: f64) {
    for k in 0..coord_j.len() {
        if k == ia || k == ib {
            continue;
        }
        let xk = coord_j[k];
        let (ma, mb) = ((ja - xk) * (ja - xk), (jb - xk) * (jb - xk));
        let (na, nb) = (d2[ia][k] - ma + mb, d2[ib][k] - mb + ma);
        d2[ia][k] = na;
        d2[k][ia] = na;
        d2[ib][k] = nb;
        d2[k][ib] = nb;
    }
}

/// Runs the descent over the placed values of the `m` new designs.
///
/// `placed_t[j][i]` is the scope-transformed value of parameter `j` and
/// new design `i`; `mirror` holds opaque parallel values (the real
/// output values) that are swapped in lockstep, so the caller's output
/// stays consistent with the refined assignment without this module
/// knowing anything about real-space transforms. `base_points` are the
/// context points of the space-filling criterion: the designs in scope
/// for every parameter, transformed to the unit cube.
pub(crate) fn polish(
    pairs: &[PairSeries],
    base_points: &[Vec<f64>],
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

    // Phase 2 refines the space-filling (AE energy) under the τ-floor
    // ladder: tier guards are per-pair, tightest tier first, each tier
    // to its own convergence. The budget is counted in distance-term
    // evaluations and is a hard cap shared across all tiers.
    let ae_before = ae_energy(&active_rows(base_points, placed_t));
    let mut ae_moves = 0usize;
    let n0 = base_points.len();
    let n = n0 + m;
    let cost2 = phase2_sweep_cost(p, m, n);
    let budget2 = phase2_budget(cost2);
    let mut evals2: u128 = 0;
    'tiers: for &tau in TAU_TIERS.iter() {
        let mut tier_sweeps: u128 = 0;
        loop {
            tier_sweeps += 1;
            // rebuild every sweep, like phase 1: fresh pair totals and
            // a fresh distance matrix avoid accumulated rounding
            let mut totals = pair_totals(pairs, placed_t, m);
            let mut d2 = distance_matrix(base_points, placed_t);
            let mut applied = 0usize;
            for j in 0..p {
                // coordinate-j values of every active point, maintained
                // in lockstep with the swaps below
                let mut coord_j: Vec<f64> = Vec::with_capacity(n);
                for point in base_points {
                    coord_j.push(point[j]);
                }
                coord_j.extend_from_slice(&placed_t[j]);
                for a in 0..m {
                    for b in (a + 1)..m {
                        if evals2 >= budget2 {
                            break 'tiers;
                        }
                        // the per-pair τ guard first: O(p), no distance
                        // terms spent. A swap in j may only change the
                        // pairs involving j; each must land at or below
                        // the active tier afterwards.
                        let (ja, jb) = (placed_t[j][a], placed_t[j][b]);
                        let mut guarded = true;
                        for &(k, side) in &affected[j] {
                            let pair = &pairs[k];
                            let (oa, ob) = match side {
                                Side::X => (placed_t[pair.b][a], placed_t[pair.b][b]),
                                Side::Y => (placed_t[pair.a][a], placed_t[pair.a][b]),
                            };
                            if totals[k].r2_with_swap(ja, jb, oa, ob) > tau {
                                guarded = false;
                                break;
                            }
                        }
                        if !guarded {
                            continue;
                        }
                        // the AE delta: the two swapped rows against every
                        // other active point
                        evals2 += 2 * (n as u128 - 2);
                        let (ia, ib) = (n0 + a, n0 + b);
                        let delta = ae_swap_delta(&d2, &coord_j, ia, ib);
                        if delta < -IMPROVE_EPS {
                            placed_t[j].swap(a, b);
                            mirror[j].swap(a, b);
                            coord_j.swap(ia, ib);
                            apply_ae_swap(&mut d2, &coord_j, ia, ib, ja, jb);
                            for &(k, side) in &affected[j] {
                                let pair = &pairs[k];
                                let (oa, ob) = match side {
                                    Side::X => (placed_t[pair.b][a], placed_t[pair.b][b]),
                                    Side::Y => (placed_t[pair.a][a], placed_t[pair.a][b]),
                                };
                                totals[k].apply_swap(ja, jb, oa, ob);
                            }
                            ae_moves += 1;
                            applied += 1;
                        }
                    }
                }
            }
            // tier convergence, tier sweep cap, or the shared budget
            if applied == 0 || tier_sweeps >= MAX_SWEEPS {
                break;
            }
        }
    }

    let score_after = assignment_score(pairs, placed_t, m);
    let ae_after = ae_energy(&active_rows(base_points, placed_t));
    PolishReport {
        moves,
        ae_moves,
        score_before,
        score_after,
        ae_before,
        ae_after,
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

    fn no_base() -> Vec<Vec<f64>> {
        Vec::new()
    }

    fn scaled_mirror(t: &[Vec<f64>]) -> Vec<Vec<f64>> {
        t.iter()
            .map(|v| v.iter().map(|x| x * 10.0).collect())
            .collect()
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
        let mut mirror = scaled_mirror(&t);
        let original_sorted: Vec<Vec<f64>> = t.iter().map(|v| sorted(v)).collect();

        let report = polish(&pairs, &no_base(), &mut t, &mut mirror);

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
        // phase 2 never leaves a pair above the loosest τ tier: this
        // instance has one pair, so the pooled score is the pair r²
        assert!(
            report.score_after <= crate::space::TAU_TIERS[2] + 1e-12,
            "the τ guard is violated: {}",
            report.score_after
        );
        // the space-filling half is monotone too
        assert!(
            report.ae_after <= report.ae_before,
            "AE worsened: {} -> {}",
            report.ae_before,
            report.ae_after
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
    fn phase_two_breaks_the_null_correlation_x() {
        // the X-pattern defect (issue #46): four designs on the line
        // y = x and four on the line y = 1 - x. The arms cancel
        // exactly, so every correlation metric reads r^2 = 0 while the
        // dependence is perfect and the space-filling is poor. Phase 1
        // cannot move (r^2 is already minimal); phase 2 must improve
        // the space-filling under the τ guard.
        let xs = vec![0.05, 0.15, 0.25, 0.35, 0.65, 0.75, 0.85, 0.95];
        let ys = vec![0.05, 0.15, 0.25, 0.35, 0.35, 0.25, 0.15, 0.05];
        let pairs = vec![pair(0, 1, Vec::new(), Vec::new())];
        let mut t = vec![xs, ys];
        let mut mirror = scaled_mirror(&t);
        let original_sorted: Vec<Vec<f64>> = t.iter().map(|v| sorted(v)).collect();

        let report = polish(&pairs, &no_base(), &mut t, &mut mirror);

        assert!(
            report.score_before < 1e-12,
            "the anti-symmetric X cancels r^2 (up to float rounding): {}",
            report.score_before
        );
        assert_eq!(report.moves, 0, "r^2 is minimal: phase 1 must not move");
        assert!(
            report.ae_moves > 0,
            "the X must have space-filling improving swaps"
        );
        assert!(
            report.ae_after < report.ae_before,
            "AE must improve: {} -> {}",
            report.ae_before,
            report.ae_after
        );
        // the per-pair τ guard holds through every phase-2 swap: one
        // pair here, so the pooled score is the pair r²
        assert!(
            report.score_after <= crate::space::TAU_TIERS[2] + 1e-12,
            "the τ guard is violated: {}",
            report.score_after
        );
        // the per-parameter multisets survive every swap
        for j in 0..2 {
            assert_eq!(sorted(&t[j]), original_sorted[j], "parameter {j} changed");
        }
        for j in 0..2 {
            for i in 0..t[j].len() {
                assert!(
                    (mirror[j][i] - t[j][i] * 10.0).abs() < 1e-12,
                    "mirror out of sync at ({j}, {i})"
                );
            }
        }
    }

    #[test]
    fn phase_two_spreads_new_designs_away_from_base_points() {
        // the X construction with one base point sitting ON the dense
        // arm of the X: the base point is a context point of the AE
        // criterion, and pushing the new designs away from it opens
        // improving swaps no r²-only pass could ever see (r² is
        // already minimal). The active set is base + new — the matrix
        // rows of the base point must take part — and the base point
        // itself never moves.
        let xs = vec![0.05, 0.15, 0.25, 0.35, 0.65, 0.75, 0.85, 0.95];
        let ys = vec![0.05, 0.15, 0.25, 0.35, 0.35, 0.25, 0.15, 0.05];
        let base = vec![vec![0.3, 0.3]];
        let pairs = vec![pair(0, 1, Vec::new(), Vec::new())];
        let mut t = vec![xs, ys];
        let mut mirror = scaled_mirror(&t);
        let original_sorted: Vec<Vec<f64>> = t.iter().map(|v| sorted(v)).collect();

        let report = polish(&pairs, &base, &mut t, &mut mirror);

        assert!(
            report.score_before < 1e-12,
            "the X cancels r^2: {}",
            report.score_before
        );
        assert_eq!(report.moves, 0, "r^2 is minimal: phase 1 must not move");
        assert!(
            report.ae_moves > 0,
            "the base point must open improving swaps"
        );
        assert!(report.ae_after < report.ae_before);
        // the per-pair τ guard holds through every phase-2 swap
        assert!(
            report.score_after <= crate::space::TAU_TIERS[2] + 1e-12,
            "the τ guard is violated: {}",
            report.score_after
        );
        for j in 0..2 {
            assert_eq!(sorted(&t[j]), original_sorted[j], "parameter {j} changed");
        }
        // the base point never moves: it is not part of placed_t
        assert_eq!(base, vec![vec![0.3, 0.3]]);
    }

    #[test]
    fn polish_keeps_an_assignment_without_improving_swaps() {
        // two designs, one pair: the only candidate swap (x of design
        // 0 and 1) flips the correlation sign, r^2 is unchanged — the
        // descent must converge with zero moves and equal scores.
        // Phase 2 cannot move either: the active set is exactly the
        // two designs, whose only distance is swap-invariant.
        let pairs = vec![pair(0, 1, Vec::new(), Vec::new())];
        let mut t = vec![vec![0.25, 0.75], vec![0.25, 0.75]];
        let mut mirror = vec![vec![2.5, 7.5], vec![2.5, 7.5]];

        let report = polish(&pairs, &no_base(), &mut t, &mut mirror);
        assert_eq!(report.moves, 0);
        assert_eq!(report.ae_moves, 0);
        assert!(
            (report.score_after - report.score_before).abs() < 1e-12,
            "scores changed without moves"
        );
        assert!((report.score_after - 1.0).abs() < 1e-9);
        assert_eq!(report.ae_before, report.ae_after);
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
    fn substituted_ae_delta_matches_a_full_recompute() {
        // three base points plus four placed designs: the matrix-fixup
        // AE delta of a candidate swap must equal the energy
        // difference of a full recompute (times the pair count, since
        // the delta is a raw term sum)
        let base = vec![vec![0.5, 0.9], vec![0.1, 0.3], vec![0.8, 0.6]];
        let t = vec![vec![0.1, 0.2, 0.8, 0.9], vec![0.1, 0.2, 0.2, 0.1]];
        let d2 = distance_matrix(&base, &t);
        let coord_j = {
            let mut c: Vec<f64> = base.iter().map(|b| b[0]).collect();
            c.extend_from_slice(&t[0]);
            c
        };
        // swap parameter 0 between placed designs 1 and 2 (active
        // rows 4 and 5)
        let (ia, ib) = (4, 5);
        let delta = ae_swap_delta(&d2, &coord_j, ia, ib);

        // full recompute with the swap applied
        let mut t_swapped = vec![t[0].clone(), t[1].clone()];
        t_swapped[0].swap(1, 2);
        let energy_before = ae_energy(&active_rows(&base, &t));
        let energy_after = ae_energy(&active_rows(&base, &t_swapped));
        let n = base.len() + 4;
        let pair_count = (n * (n - 1) / 2) as f64;
        assert!(
            (delta - (energy_after - energy_before) * pair_count).abs() < 1e-9,
            "delta {delta} vs recomputed {}",
            (energy_after - energy_before) * pair_count
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
    fn phase2_sweep_cost_counts_distance_terms() {
        // p * C(m, 2) candidates, each 2 * (n - 2) distance terms
        assert_eq!(phase2_sweep_cost(3, 8, 8), 3 * 28 * 2 * 6);
        assert_eq!(phase2_sweep_cost(2, 4, 7), 2 * 6 * 2 * 5);
        // no swap can change the energy
        assert_eq!(phase2_sweep_cost(1, 8, 8), 0, "one parameter");
        assert_eq!(phase2_sweep_cost(3, 1, 4), 0, "one movable design");
        assert_eq!(phase2_sweep_cost(2, 2, 2), 0, "two-point set");
    }

    #[test]
    fn phase2_budget_is_a_shared_hard_cap_across_the_ladder() {
        // any nonzero sweep cost buys the full budget: the ladder's
        // tiers share it, each tier capped at MAX_SWEEPS sweeps
        assert_eq!(phase2_budget(1), EVAL_BUDGET);
        assert_eq!(phase2_budget(EVAL_BUDGET), EVAL_BUDGET);
        assert_eq!(phase2_budget(EVAL_BUDGET * 100), EVAL_BUDGET);
        // nothing to evaluate: no budget at all
        assert_eq!(phase2_budget(0), 0);
    }

    #[test]
    fn polish_is_deterministic() {
        let xs: Vec<f64> = [0.05, 0.25, 0.35, 0.55, 0.75, 0.95].to_vec();
        let pairs = vec![pair(0, 1, vec![0.2, 0.7], vec![0.6, 0.3])];
        let base = vec![vec![0.4, 0.1]];
        let build = || (vec![xs.clone(), xs.clone()], vec![xs.clone(), xs.clone()]);

        let (mut t1, mut m1) = build();
        let (mut t2, mut m2) = build();
        let r1 = polish(&pairs, &base, &mut t1, &mut m1);
        let r2 = polish(&pairs, &base, &mut t2, &mut m2);
        assert_eq!(r1, r2);
        assert_eq!(t1, t2);
        assert_eq!(m1, m2);
    }
}
