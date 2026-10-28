//! Space-filling criterion (issue #46): Audze–Eglais potential energy
//! and the τ-floor rules that combine it with the correlation
//! objective.
//!
//! The pooled pair r² is structurally blind to dependence with zero
//! linear correlation: an X-shaped pair scatter — two crossing lines —
//! reports r² ≈ 0 on every pair while being perfectly dependent, and
//! within the r² ≈ 0 manifold the correlation objective is flat, so
//! the landing point there is trajectory-determined, not
//! quality-determined. This module adds the second criterion — a
//! space-filling energy that is minimized when the points spread out —
//! and the rules that combine the two.
//!
//! The combination is not an ε-tolerance on r². The r² landscape over
//! swap assignments is rugged in steps of order `1/m`: from an r² ≈ 0
//! assignment, virtually every single swap lands `1/m`-scale r² away,
//! so any tolerance tighter than that — and the `1e-9` initially
//! considered is seven orders tighter at m = 100 — freezes the search
//! inside the null-correlation manifold, exactly where the X lives.
//! The rules below instead treat every r² below a practical floor τ
//! as the same state ("null correlation", far inside the |r| < 0.3
//! acceptance band of the project's judgment rules): below the floor
//! the criteria trade freely, above it correlation dominates
//! outright.

/// Regularization of the AE term, keeping `1/(d² + δ)` finite for
/// near-coincident points. Small enough never to mask a real
/// improvement on the unit cube, where meaningful squared distances
/// are orders of magnitude larger.
pub const AE_DELTA: f64 = 1e-12;

/// The practical per-pair null floor of the selection comparator: a
/// pair with r² ≤ τ is a null pair for selection purposes. τ = 1e-2
/// means |r| ≤ 0.1 — zero visible correlation, well inside the
/// project's |r| < 0.3 acceptance rule, and an order below the
/// ≈ 1/(m-1) r² a random pairing averages at m = 100.
pub const SELECTION_TAU: f64 = 1e-2;

/// The ascending per-pair r² guards of the polish phase-2 ladder,
/// tightest first. Each tier descends the space-filling energy to
/// convergence under its own guard; the ladder then proceeds to the
/// next tier. The tight tiers give the search its r² quality where
/// the swap neighborhood is dense enough to support them (m ≳ 30 at
/// τ = 1e-4), while the loosest tier — identical to
/// [`SELECTION_TAU`] — guarantees the search is not frozen at small
/// m, where single-swap r² jumps are too large for anything tighter.
pub const TAU_TIERS: [f64; 3] = [1e-9, 1e-4, SELECTION_TAU];

/// One Audze–Eglais term of a squared distance `d²`: `1/(d² + δ)`.
pub fn ae_term(d2: f64) -> f64 {
    1.0 / (d2 + AE_DELTA)
}

/// Audze–Eglais potential energy of a point set: the mean of
/// [`ae_term`] over all unordered point pairs. Rows are designs, each
/// row a full unit-cube vector. `0.0` below two points. Lower means
/// better spread; the mean — not the sum — keeps the number
/// comparable across design counts.
pub fn ae_energy(points: &[Vec<f64>]) -> f64 {
    let n = points.len();
    if n < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    for a in 0..n {
        for b in (a + 1)..n {
            let mut d2 = 0.0;
            for (&x, &y) in points[a].iter().zip(points[b].iter()) {
                let d = x - y;
                d2 += d * d;
            }
            sum += ae_term(d2);
        }
    }
    sum / ((n * (n - 1) / 2) as f64)
}

/// The saturated r² key of one candidate: the sum over pairs of
/// `max(r², `[`SELECTION_TAU`]`)`. Every fully-null pair contributes
/// the identical saturated term, so candidates whose pairs are all
/// null tie exactly on this key; a single pair above the floor raises
/// the key by its full excess. Callers compare candidates on
/// `(saturated key, AE energy)` lexicographically via
/// [`saturated_better`]: correlation dominates above the floor,
/// space-filling decides below it.
pub fn sat_key(pair_r2s: &[f64]) -> f64 {
    pair_r2s
        .iter()
        .map(|&r2| r2.max(SELECTION_TAU))
        .sum::<f64>()
}

/// The τ-floor comparator: candidate `a` beats incumbent `b` when its
/// saturated r² key ([`sat_key`]) is lower, or when the two keys tie
/// — both candidates null on every pair — and its space-filling
/// energy is strictly lower. Correlation outranks space-filling
/// outright above the floor; below the floor the r² differences are
/// practically meaningless and space-filling decides. The key is
/// expected precomputed via [`sat_key`]; equal multisets of
/// null-pair-only keys are exactly equal floats (every term saturates
/// to the same constant), so the tie test is exact.
pub fn saturated_better(key_a: f64, ae_a: f64, key_b: f64, ae_b: f64) -> bool {
    if key_a < key_b {
        return true;
    }
    if key_b < key_a {
        return false;
    }
    ae_a < ae_b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ae_term_regularizes_coincident_points() {
        // coincident points stay finite: 1/(0 + δ)
        assert!((ae_term(0.0) - 1.0 / AE_DELTA).abs() < 1e-27);
        assert!((ae_term(0.25) - 1.0 / (0.25 + AE_DELTA)).abs() < 1e-15);
    }

    #[test]
    fn ae_energy_is_the_mean_term_over_all_pairs() {
        // three points on a line: distances² 0.01 and 0.04 between the
        // neighbours and 0.09 across — the energy is the mean of the
        // three terms
        let points = vec![vec![0.1, 0.0], vec![0.2, 0.0], vec![0.4, 0.0]];
        let expected = (ae_term(0.01) + ae_term(0.04) + ae_term(0.09)) / 3.0;
        assert!((ae_energy(&points) - expected).abs() < 1e-12);
    }

    #[test]
    fn ae_energy_spread_points_score_lower_than_clustered() {
        // same two-value multisets, different pairing: the clustered
        // pairing puts two points at distance² 0.01, the spread pairing
        // never does
        let clustered = vec![
            vec![0.1, 0.1],
            vec![0.2, 0.2],
            vec![0.8, 0.2],
            vec![0.9, 0.1],
        ];
        let spread = vec![
            vec![0.1, 0.2],
            vec![0.2, 0.1],
            vec![0.8, 0.2],
            vec![0.9, 0.1],
        ];
        assert!(ae_energy(&spread) < ae_energy(&clustered));
    }

    #[test]
    fn ae_energy_is_zero_below_two_points() {
        assert_eq!(ae_energy(&[]), 0.0);
        assert_eq!(ae_energy(&[vec![0.5, 0.5]]), 0.0);
    }

    #[test]
    fn sat_key_saturates_null_pairs_and_summarizes_excess() {
        // every pair below the floor contributes the floor itself
        assert!((sat_key(&[0.0, 1e-30, 5e-3]) - 3.0 * SELECTION_TAU).abs() < 1e-15);
        // a pair above the floor contributes its full r²
        assert!((sat_key(&[SELECTION_TAU, 0.25]) - (SELECTION_TAU + 0.25)).abs() < 1e-15);
        // empty pair sets saturate to zero
        assert_eq!(sat_key(&[]), 0.0);
    }

    #[test]
    fn saturated_order_dominates_above_the_floor_and_breaks_ties_on_ae() {
        let tie = 2.0 * SELECTION_TAU;
        // above the floor the key decides outright — even against much
        // better space-filling
        assert!(saturated_better(tie, 100.0, tie + 1e-6, 0.0));
        assert!(!saturated_better(tie + 1e-6, 0.0, tie, 100.0));
        // two fully-null keys tie exactly: AE decides
        assert!(saturated_better(tie, 1.0, tie, 2.0));
        assert!(!saturated_better(tie, 2.0, tie, 1.0));
        // equal on both: not better (strict improvement required)
        assert!(!saturated_better(tie, 1.0, tie, 1.0));
    }
}
