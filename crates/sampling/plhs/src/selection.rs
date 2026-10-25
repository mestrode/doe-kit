//! Combination selection: choose the assignment of `m` new designs to empty
//! bins — one distinct empty bin per parameter and design — with the lowest
//! score.
//!
//! Two strategies:
//!
//! - **Exact enumeration** when the number of assignments
//!   `prod_j P(k_j, m)` does not exceed the configured cap; every assignment
//!   is scored and the best (ties resolved by the first in lexicographic
//!   order) wins.
//! - **Greedy fallback** above the cap: designs are placed one at a time,
//!   each choosing the unused bin combination that minimizes the score of
//!   all designs placed so far (again deterministic, ties to the
//!   lexicographically first combination).

use crate::error::{Error, Result};
use crate::scoring::{PairSeries, assignment_score, assignment_score_with};

/// Upper bound on the empty-bin cross product searched per greedy step.
const GREEDY_CROSS_CAP: u128 = 10_000_000;

/// Up to this many placed designs, the greedy fallback scores every leaf
/// with the reference two-pass scorer ([`assignment_score_with`]); beyond
/// it, the incremental O(1) scorer ([`PairSums`]) takes over. The limit
/// keeps the historical floating-point behaviour — which the golden
/// fixtures pin — bit-identical for small pools while making large
/// `--explore` runs tractable.
const REFERENCE_SCORING_PLACED_LIMIT: usize = 10;

/// Candidate values of the empty bins of one parameter.
#[derive(Debug, Clone)]
pub struct BinCandidates {
    /// Empty bin indices, ascending.
    pub bins: Vec<usize>,
    /// Scope-transformed candidate value per empty bin (aligned with `bins`).
    pub t: Vec<f64>,
    /// Real-space candidate value per empty bin (aligned with `bins`).
    pub values: Vec<f64>,
}

/// The strategy a [`select`] run used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Exact enumeration of every assignment (at or below the cap).
    Exact,
    /// Greedy sequential selection (above the cap).
    Greedy,
}

impl SelectionStrategy {
    /// Stable lowercase name for reports: `"exact"` / `"greedy"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Greedy => "greedy",
        }
    }
}

/// The chosen assignment.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Per parameter: the chosen candidate index (into [`BinCandidates`])
    /// for each new design.
    pub picks: Vec<Vec<usize>>,
    /// Score of the chosen assignment.
    pub score: f64,
    /// The strategy that produced this selection.
    pub strategy: SelectionStrategy,
}

/// Selects the best assignment of `m` new designs to the empty bins.
///
/// `candidates` holds the empty-bin candidates per parameter; `pairs` holds
/// the in-scope series used for scoring. `cap` is the maximum number of
/// assignments to enumerate exactly.
pub fn select(
    m: usize,
    pairs: &[PairSeries],
    candidates: &[BinCandidates],
    cap: u64,
) -> Result<Selection> {
    for (j, c) in candidates.iter().enumerate() {
        if c.bins.len() < m {
            return Err(Error::Invariant(format!(
                "parameter index {j}: {} empty bin(s) for {m} new design(s); \
                 the pigeonhole invariant (>= m empty bins) is violated",
                c.bins.len()
            )));
        }
    }
    let total = total_assignments(m, candidates);
    let run = if total <= u128::from(cap) {
        enumerate(m, pairs, candidates)
    } else {
        greedy(m, pairs, candidates)
    };
    run.map(|mut sel| {
        sel.strategy = if total <= u128::from(cap) {
            SelectionStrategy::Exact
        } else {
            SelectionStrategy::Greedy
        };
        sel
    })
}

/// Number of assignments: the product over parameters of the ordered
/// selections of `m` distinct bins out of `k_j` empty bins. Saturates at
/// [`u128::MAX`] instead of overflowing.
fn total_assignments(m: usize, candidates: &[BinCandidates]) -> u128 {
    saturating_product(candidates.iter().map(|c| perm_count(c.bins.len(), m)))
}

/// Saturating product of an iterator of factors: multiplies without
/// overflowing, stopping at [`u128::MAX`]. `Iterator::product` would panic
/// on overflow as soon as two factors saturate (e.g. `40! * 40!`).
fn saturating_product(factors: impl Iterator<Item = u128>) -> u128 {
    factors.fold(1u128, u128::saturating_mul)
}

/// Unnormalized cross product of the empty-bin candidate sets: the number
/// of bin combinations one greedy step searches. Saturating for the same
/// reason as [`total_assignments`].
fn cross_product(candidates: &[BinCandidates]) -> u128 {
    saturating_product(candidates.iter().map(|c| c.bins.len() as u128))
}

/// `P(k, m) = k * (k-1) * ... * (k-m+1)`, saturating.
fn perm_count(k: usize, m: usize) -> u128 {
    let mut total = 1u128;
    for i in 0..m {
        if k <= i {
            return 0;
        }
        total = total.saturating_mul(u128::try_from(k - i).unwrap_or(u128::MAX));
    }
    total
}

// --- exact enumeration ------------------------------------------------------

struct Enumerator<'a> {
    m: usize,
    pairs: &'a [PairSeries],
    candidates: &'a [BinCandidates],
    /// Current picks: `picks[param][design]` = candidate index.
    picks: Vec<Vec<usize>>,
    /// Scratch: transformed candidate values per param/design.
    cand: Vec<Vec<f64>>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    best: Option<Selection>,
}

fn enumerate(m: usize, pairs: &[PairSeries], candidates: &[BinCandidates]) -> Result<Selection> {
    let p = candidates.len();
    let mut state = Enumerator {
        m,
        pairs,
        candidates,
        picks: vec![vec![0; m]; p],
        cand: vec![vec![0.0; m]; p],
        xs: Vec::new(),
        ys: Vec::new(),
        best: None,
    };
    state.rec_param(0);
    state
        .best
        .ok_or_else(|| Error::Invariant("empty search space".to_owned()))
}

impl Enumerator<'_> {
    fn rec_param(&mut self, j: usize) {
        if j == self.candidates.len() {
            self.score_leaf();
            return;
        }
        let k = self.candidates[j].bins.len();
        let mut used = vec![false; k];
        self.rec_slot(j, 0, &mut used);
    }

    fn rec_slot(&mut self, j: usize, slot: usize, used: &mut Vec<bool>) {
        if slot == self.m {
            self.rec_param(j + 1);
            return;
        }
        let k = self.candidates[j].bins.len();
        for idx in 0..k {
            if used[idx] {
                continue;
            }
            used[idx] = true;
            self.picks[j][slot] = idx;
            self.cand[j][slot] = self.candidates[j].t[idx];
            self.rec_slot(j, slot + 1, used);
            used[idx] = false;
        }
    }

    fn score_leaf(&mut self) {
        let score =
            assignment_score_with(self.pairs, &self.cand, self.m, &mut self.xs, &mut self.ys);
        let better = self.best.as_ref().is_none_or(|b| score < b.score);
        if better {
            self.best = Some(Selection {
                picks: self.picks.clone(),
                score,
                strategy: SelectionStrategy::Exact,
            });
        }
    }
}

// --- greedy fallback ---------------------------------------------------------

/// Running raw sums of one pair's series — the in-scope base values plus
/// the designs placed so far — updated incrementally in placement order so
/// that scoring one more design costs O(1) instead of O(n).
struct PairSums {
    /// Number of samples accumulated so far.
    len: usize,
    sx: f64,
    sy: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

impl PairSums {
    /// Sums of a base series (the pair's shared in-scope designs).
    fn new(xs: &[f64], ys: &[f64]) -> Self {
        let mut sums = PairSums {
            len: 0,
            sx: 0.0,
            sy: 0.0,
            sxx: 0.0,
            syy: 0.0,
            sxy: 0.0,
        };
        for (&x, &y) in xs.iter().zip(ys.iter()) {
            sums.push(x, y);
        }
        sums
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

    /// Squared Pearson correlation of the accumulated series plus one
    /// more sample `(x, y)`, in O(1) from the running raw sums.
    ///
    /// Mirrors the conventions of [`crate::scoring::pearson`]: `0.0` for
    /// fewer than two samples and for zero variance on either side; the
    /// coefficient is clamped before squaring.
    fn r2_with(&self, x: f64, y: f64) -> f64 {
        let n = self.len + 1;
        if n < 2 {
            return 0.0;
        }
        let (sx, sy) = (self.sx + x, self.sy + y);
        let nf = n as f64;
        let cxx = self.sxx + x * x - sx * sx / nf;
        let cyy = self.syy + y * y - sy * sy / nf;
        let cxy = self.sxy + x * y - sx * sy / nf;
        if cxx <= 0.0 || cyy <= 0.0 {
            return 0.0;
        }
        let r = (cxy / (cxx * cyy).sqrt()).clamp(-1.0, 1.0);
        r * r
    }
}

/// Greedy score of placing design `design` on top of the accumulated
/// `sums`: the sum of squared pair correlations over all pairs, each in
/// O(1). `cand[j][design]` holds the candidate value under trial.
fn incremental_score(
    pairs: &[PairSeries],
    sums: &[PairSums],
    cand: &[Vec<f64>],
    design: usize,
) -> f64 {
    let mut score = 0.0;
    for (pair, sums) in pairs.iter().zip(sums.iter()) {
        score += sums.r2_with(cand[pair.a][design], cand[pair.b][design]);
    }
    score
}

struct Greedy<'a> {
    pairs: &'a [PairSeries],
    candidates: &'a [BinCandidates],
    /// Running sums per pair, aligned with `pairs`.
    sums: Vec<PairSums>,
    used: Vec<Vec<bool>>,
    picks: Vec<Vec<usize>>,
    cand: Vec<Vec<f64>>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// The new design currently being placed.
    design: usize,
    /// The bin combination currently being tried.
    tuple: Vec<usize>,
    best: Option<(Vec<usize>, f64)>,
}

fn greedy(m: usize, pairs: &[PairSeries], candidates: &[BinCandidates]) -> Result<Selection> {
    let cross = cross_product(candidates);
    if cross > GREEDY_CROSS_CAP {
        return Err(Error::Invalid(format!(
            "the greedy fallback would have to search {cross} bin combinations; \
             reduce the number of empty bins (e.g. narrow the scopes) or new designs"
        )));
    }
    let p = candidates.len();
    let mut state = Greedy {
        pairs,
        candidates,
        sums: pairs
            .iter()
            .map(|pair| PairSums::new(&pair.xs, &pair.ys))
            .collect(),
        used: candidates
            .iter()
            .map(|c| vec![false; c.bins.len()])
            .collect(),
        picks: vec![vec![0; m]; p],
        cand: vec![vec![0.0; m]; p],
        xs: Vec::new(),
        ys: Vec::new(),
        design: 0,
        tuple: vec![0; p],
        best: None,
    };
    for i in 0..m {
        state.design = i;
        state.best = None;
        state.rec(0);
        let (chosen, _) = state
            .best
            .take()
            .ok_or_else(|| Error::Invariant("empty search space".to_owned()))?;
        state.commit(&chosen, i);
    }
    let score = assignment_score(state.pairs, &state.cand, m);
    Ok(Selection {
        picks: state.picks.clone(),
        score,
        strategy: SelectionStrategy::Greedy,
    })
}

impl Greedy<'_> {
    fn rec(&mut self, j: usize) {
        if j == self.candidates.len() {
            for jj in 0..j {
                self.cand[jj][self.design] = self.candidates[jj].t[self.tuple[jj]];
            }
            let placed = self.design + 1;
            let score = if placed <= REFERENCE_SCORING_PLACED_LIMIT {
                assignment_score_with(self.pairs, &self.cand, placed, &mut self.xs, &mut self.ys)
            } else {
                incremental_score(self.pairs, &self.sums, &self.cand, self.design)
            };
            let better = self.best.as_ref().is_none_or(|(_, s)| score < *s);
            if better {
                self.best = Some((self.tuple.clone(), score));
            }
            return;
        }
        for idx in 0..self.candidates[j].bins.len() {
            if self.used[j][idx] {
                continue;
            }
            self.tuple[j] = idx;
            self.rec(j + 1);
        }
    }

    /// Fixes the chosen bin combination for design `i` and folds the
    /// placed values into the running pair sums.
    fn commit(&mut self, chosen: &[usize], design: usize) {
        for (j, &c) in chosen.iter().enumerate() {
            self.used[j][c] = true;
            self.picks[j][design] = c;
            self.cand[j][design] = self.candidates[j].t[c];
        }
        for (pair, sums) in self.pairs.iter().zip(self.sums.iter_mut()) {
            let (x, y) = (self.cand[pair.a][design], self.cand[pair.b][design]);
            sums.push(x, y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(pairs_of_bins: &[(Vec<usize>, f64)]) -> Vec<BinCandidates> {
        pairs_of_bins
            .iter()
            .map(|(bins, base)| {
                let t: Vec<f64> = bins
                    .iter()
                    .map(|&b| (b as f64 + 0.5) / 10.0 + *base)
                    .collect();
                BinCandidates {
                    bins: bins.clone(),
                    t: t.clone(),
                    values: t,
                }
            })
            .collect()
    }

    fn pair_series(a: usize, b: usize, xs: Vec<f64>, ys: Vec<f64>) -> PairSeries {
        PairSeries { a, b, xs, ys }
    }

    #[test]
    fn select_reports_the_strategy_it_used() {
        // 2 params, 3 empty bins each, m = 2: P(3, 2)^2 = 36 <= cap
        let cands = candidates(&[(vec![1, 4, 7], 0.0), (vec![2, 5, 8], 0.0)]);
        let pairs = vec![pair_series(0, 1, vec![0.1, 0.4, 0.9], vec![0.9, 0.6, 0.1])];
        let sel = select(2, &pairs, &cands, 1_000_000).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Exact);
        assert_eq!(sel.strategy.name(), "exact");

        // the same setup with cap 0 always exceeds the cap: greedy runs
        let sel = select(2, &pairs, &cands, 0).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Greedy);
        assert_eq!(sel.strategy.name(), "greedy");
    }

    #[test]
    fn exact_selection_enforces_distinct_bins() {
        // 2 params, 3 empty bins each, m = 2
        let cands = candidates(&[(vec![1, 4, 7], 0.0), (vec![2, 5, 8], 0.0)]);
        let pairs = vec![pair_series(0, 1, vec![0.1, 0.4, 0.9], vec![0.9, 0.6, 0.1])];
        let sel = select(2, &pairs, &cands, 1_000_000).unwrap();
        for j in 0..2 {
            assert_ne!(sel.picks[j][0], sel.picks[j][1]);
        }
    }

    #[test]
    fn greedy_matches_enumeration_when_both_feasible() {
        let cands = candidates(&[(vec![1, 4, 7], 0.0), (vec![2, 5, 8], 0.05)]);
        let pairs = vec![pair_series(
            0,
            1,
            vec![0.1, 0.4, 0.55, 0.9],
            vec![0.9, 0.6, 0.45, 0.1],
        )];
        let exact = select(2, &pairs, &cands, 1_000_000).unwrap();
        let greedy_sel = select(2, &pairs, &cands, 1).unwrap();
        assert!((exact.score - greedy_sel.score).abs() < 1e-12);
    }

    #[test]
    fn cap_zero_always_greedy() {
        // enumeration would consider P(4,2)^2 = 144 assignments; cap 0 forces greedy
        let cands = candidates(&[(vec![0, 1, 4, 7], 0.0), (vec![2, 3, 5, 8], 0.0)]);
        let pairs = vec![pair_series(
            0,
            1,
            vec![0.1, 0.4, 0.55, 0.9],
            vec![0.9, 0.6, 0.45, 0.1],
        )];
        let sel = select(2, &pairs, &cands, 0).unwrap();
        for j in 0..2 {
            assert_ne!(sel.picks[j][0], sel.picks[j][1]);
        }
    }

    #[test]
    fn rejects_fewer_empty_bins_than_designs() {
        let cands = candidates(&[(vec![3], 0.0)]);
        let pairs: Vec<PairSeries> = vec![];
        assert!(select(2, &pairs, &cands, 1_000_000).is_err());
    }

    #[test]
    fn perm_count_matches_formula() {
        assert_eq!(perm_count(5, 2), 20);
        assert_eq!(perm_count(3, 3), 6);
        assert_eq!(perm_count(2, 5), 0);
    }

    #[test]
    fn total_assignments_saturates_instead_of_panicking() {
        // regression for `plhs --explore 1000` on a fresh pool: each
        // parameter's permutation count saturates at u128::MAX and the
        // non-saturating `Iterator::product` over two saturated factors
        // overflows and panics. 2 parameters x 40 empty bins already
        // saturate: 40! > u128::MAX.
        let forty: Vec<usize> = (0..40).collect();
        let cands = candidates(&[(forty.clone(), 0.0), (forty, 0.0)]);
        assert_eq!(perm_count(40, 40), u128::MAX);
        assert_eq!(total_assignments(40, &cands), u128::MAX);
        // the unnormalized cross product of the same candidates
        assert_eq!(cross_product(&cands), 40 * 40);
    }

    #[test]
    fn saturating_product_stops_at_u128_max() {
        assert_eq!(saturating_product(std::iter::empty()), 1);
        assert_eq!(saturating_product([3u128, 4, 5].into_iter()), 60);
        // two saturated factors no longer panic
        assert_eq!(saturating_product([u128::MAX, 2, 3].into_iter()), u128::MAX);
    }

    #[test]
    fn incremental_score_matches_the_reference_scorer() {
        // a pair with a non-empty base series plus 30 placed designs: the
        // O(1) incremental scorer must track the two-pass reference
        // scorer to within float rounding
        let xs = vec![0.1, 0.4, 0.55, 0.9];
        let ys = vec![0.9, 0.6, 0.45, 0.1];
        let pairs = vec![pair_series(0, 1, xs, ys)];
        let cand: Vec<Vec<f64>> = (0..2)
            .map(|j| {
                (0..30)
                    .map(|i| (((i * 7 + j * 11) % 30) as f64 + 0.5) / 31.0)
                    .collect()
            })
            .collect();
        let mut sums = vec![PairSums::new(&pairs[0].xs, &pairs[0].ys)];
        let mut rx = Vec::new();
        let mut ry = Vec::new();
        for i in 0..30 {
            let incremental = incremental_score(&pairs, &sums, &cand, i);
            let reference = assignment_score_with(&pairs, &cand, i + 1, &mut rx, &mut ry);
            assert!(
                (incremental - reference).abs() < 1e-9,
                "design {i}: incremental={incremental} reference={reference}"
            );
            sums[0].push(cand[0][i], cand[1][i]);
        }
    }

    #[test]
    fn greedy_scores_beyond_the_reference_limit_incrementally() {
        // 15 designs on a fresh pool (15 empty bins per parameter):
        // P(15, 15)^2 saturates far above the cap, so greedy runs; placed
        // counts 11..=15 take the incremental path
        let fifteen: Vec<usize> = (0..15).collect();
        let cands = candidates(&[(fifteen.clone(), 0.0), (fifteen, 0.01)]);
        let pairs = vec![pair_series(
            0,
            1,
            vec![0.1, 0.4, 0.55, 0.9],
            vec![0.9, 0.6, 0.45, 0.1],
        )];
        let sel = select(15, &pairs, &cands, 0).unwrap();
        for j in 0..2 {
            let mut bins: Vec<usize> = sel.picks[j].clone();
            bins.sort_unstable();
            bins.dedup();
            assert_eq!(bins.len(), 15, "parameter {j}: designs share bins");
        }
        assert!(sel.score.is_finite() && sel.score >= 0.0);
    }
}
