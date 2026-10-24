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

/// The chosen assignment.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Per parameter: the chosen candidate index (into [`BinCandidates`])
    /// for each new design.
    pub picks: Vec<Vec<usize>>,
    /// Score of the chosen assignment.
    pub score: f64,
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
    if total <= u128::from(cap) {
        enumerate(m, pairs, candidates)
    } else {
        greedy(m, pairs, candidates)
    }
}

/// Number of assignments: the product over parameters of the ordered
/// selections of `m` distinct bins out of `k_j` empty bins.
fn total_assignments(m: usize, candidates: &[BinCandidates]) -> u128 {
    candidates
        .iter()
        .map(|c| perm_count(c.bins.len(), m))
        .product()
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
            });
        }
    }
}

// --- greedy fallback ---------------------------------------------------------

struct Greedy<'a> {
    pairs: &'a [PairSeries],
    candidates: &'a [BinCandidates],
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
    let cross: u128 = candidates.iter().map(|c| c.bins.len() as u128).product();
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
        for (j, &c) in chosen.iter().enumerate() {
            state.used[j][c] = true;
            state.picks[j][i] = c;
            state.cand[j][i] = state.candidates[j].t[c];
        }
    }
    let score = assignment_score(state.pairs, &state.cand, m);
    Ok(Selection {
        picks: state.picks.clone(),
        score,
    })
}

impl Greedy<'_> {
    fn rec(&mut self, j: usize) {
        if j == self.candidates.len() {
            for jj in 0..j {
                self.cand[jj][self.design] = self.candidates[jj].t[self.tuple[jj]];
            }
            let score = assignment_score_with(
                self.pairs,
                &self.cand,
                self.design + 1,
                &mut self.xs,
                &mut self.ys,
            );
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
}
