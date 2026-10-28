//! Correlation scoring of candidate assignments.

/// Pearson correlation coefficient of two series.
///
/// Returns `0.0` when either side has zero variance (the correlation is
/// undefined; the pair then contributes nothing to the score) and when
/// fewer than two samples are given.
pub fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
    debug_assert_eq!(xs.len(), ys.len());
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let sx: f64 = xs.iter().sum();
    let sy: f64 = ys.iter().sum();
    let (mx, my) = (sx / n as f64, sy / n as f64);
    let (mut sxx, mut syy, mut sxy) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in xs.iter().zip(ys.iter()) {
        let (dx, dy) = (x - mx, y - my);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    if sxx == 0.0 || syy == 0.0 {
        return 0.0;
    }
    (sxy / (sxx * syy).sqrt()).clamp(-1.0, 1.0)
}

/// In-scope series of one unordered parameter pair: the scope-transformed
/// values of the designs that are in scope for both parameters.
#[derive(Debug, Clone)]
pub struct PairSeries {
    /// Index of the first parameter.
    pub a: usize,
    /// Index of the second parameter.
    pub b: usize,
    /// Transformed values of parameter `a`, aligned with `ys`.
    pub xs: Vec<f64>,
    /// Transformed values of parameter `b`, aligned with `xs`.
    pub ys: Vec<f64>,
}

/// Score of a (partial) assignment: the sum of squared Pearson correlations
/// over all parameter pairs, with the first `placed` candidate designs
/// appended to each pair's series.
///
/// `cand[j][i]` is the scope-transformed candidate value for parameter `j`
/// and new design `i`.
pub fn assignment_score(pairs: &[PairSeries], cand: &[Vec<f64>], placed: usize) -> f64 {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    assignment_score_with(pairs, cand, placed, &mut xs, &mut ys)
}

/// Like [`assignment_score`], but reuses caller-provided scratch buffers to
/// avoid per-call allocations (the selection search calls this in its inner
/// loop).
pub fn assignment_score_with(
    pairs: &[PairSeries],
    cand: &[Vec<f64>],
    placed: usize,
    xs: &mut Vec<f64>,
    ys: &mut Vec<f64>,
) -> f64 {
    let mut r2s = Vec::new();
    assignment_pair_r2s_with(pairs, cand, placed, xs, ys, &mut r2s);
    r2s.iter().sum()
}

/// Per-pair squared Pearson correlations of a (partial) assignment,
/// aligned with `pairs` — the ingredients of both the pooled score
/// (their sum, [`assignment_score_with`]) and the saturated selection
/// key (their per-pair τ floor, `plhs::space::sat_key`). Same series
/// construction and floating-point behaviour as the pooled scorer.
pub fn assignment_pair_r2s_with(
    pairs: &[PairSeries],
    cand: &[Vec<f64>],
    placed: usize,
    xs: &mut Vec<f64>,
    ys: &mut Vec<f64>,
    out: &mut Vec<f64>,
) {
    out.clear();
    for pair in pairs {
        xs.clear();
        ys.clear();
        xs.extend_from_slice(&pair.xs);
        ys.extend_from_slice(&pair.ys);
        xs.extend_from_slice(&cand[pair.a][..placed]);
        ys.extend_from_slice(&cand[pair.b][..placed]);
        let r = pearson(xs, ys);
        out.push(r * r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearson_perfect_correlations() {
        let xs = [1.0, 2.0, 3.0, 4.0];
        let ys = [2.0, 4.0, 6.0, 8.0];
        assert!((pearson(&xs, &ys) - 1.0).abs() < 1e-12);
        let ys2 = [8.0, 6.0, 4.0, 2.0];
        assert!((pearson(&xs, &ys2) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn pearson_zero_variance_contributes_zero() {
        let xs = [1.0, 1.0, 1.0, 1.0];
        let ys = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(pearson(&xs, &ys), 0.0);
    }

    #[test]
    fn pearson_known_value() {
        // classic small dataset
        let xs = [1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = [2.0, 1.0, 4.0, 3.0, 5.0];
        let r = pearson(&xs, &ys);
        assert!((r - 0.8).abs() < 1e-12, "r={r}");
    }

    #[test]
    fn pearson_single_sample_is_zero() {
        assert_eq!(pearson(&[1.0], &[2.0]), 0.0);
    }

    #[test]
    fn assignment_pair_r2s_sum_to_the_pooled_score() {
        let pairs = vec![
            PairSeries {
                a: 0,
                b: 1,
                xs: vec![0.1, 0.5, 0.9],
                ys: vec![0.1, 0.6, 0.9],
            },
            PairSeries {
                a: 0,
                b: 2,
                xs: vec![0.1, 0.5, 0.9],
                ys: vec![0.9, 0.4, 0.1],
            },
        ];
        let cand = vec![vec![0.3, 0.7], vec![0.3, 0.7], vec![0.7, 0.3]];
        let pooled = assignment_score(&pairs, &cand, 2);
        let (mut xs, mut ys, mut r2s) = (Vec::new(), Vec::new(), Vec::new());
        assignment_pair_r2s_with(&pairs, &cand, 2, &mut xs, &mut ys, &mut r2s);
        assert_eq!(r2s.len(), 2);
        assert_eq!(r2s.iter().sum::<f64>(), pooled);
        // the per-pair values saturate into the selection key
        let key = crate::space::sat_key(&r2s);
        assert!(
            (key - r2s
                .iter()
                .map(|&r2| r2.max(crate::space::SELECTION_TAU))
                .sum::<f64>())
            .abs()
                < 1e-15
        );
    }

    #[test]
    fn assignment_score_sums_squared_pair_correlations() {
        let pairs = vec![
            PairSeries {
                a: 0,
                b: 1,
                xs: vec![0.1, 0.5, 0.9],
                ys: vec![0.1, 0.6, 0.9],
            },
            PairSeries {
                a: 0,
                b: 2,
                xs: vec![0.1, 0.5, 0.9],
                ys: vec![0.9, 0.4, 0.1],
            },
        ];
        // candidates: cand[param][design]
        let cand = vec![
            vec![0.3, 0.7], // param 0
            vec![0.3, 0.7], // param 1, perfectly correlated with param 0
            vec![0.7, 0.3], // param 2, perfectly anti-correlated
        ];
        // the wiring under test: each pair gets its own two candidates appended
        let expected = pearson(&[0.1, 0.5, 0.9, 0.3, 0.7], &[0.1, 0.6, 0.9, 0.3, 0.7]).powi(2)
            + pearson(&[0.1, 0.5, 0.9, 0.3, 0.7], &[0.9, 0.4, 0.1, 0.7, 0.3]).powi(2);
        let score = assignment_score(&pairs, &cand, 2);
        assert!((score - expected).abs() < 1e-12, "score={score}");
        assert!(score >= 1.0, "the perfect pair alone contributes 1.0");

        // partial placement appends only the first candidate
        let partial_expected = pearson(&[0.1, 0.5, 0.9, 0.3], &[0.1, 0.6, 0.9, 0.3]).powi(2)
            + pearson(&[0.1, 0.5, 0.9, 0.3], &[0.9, 0.4, 0.1, 0.7]).powi(2);
        let partial = assignment_score(&pairs, &cand, 1);
        assert!(
            (partial - partial_expected).abs() < 1e-12,
            "partial={partial}"
        );
        assert!((partial - score).abs() > 1e-9);
    }
}
