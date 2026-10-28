//! Combination selection: choose the assignment of `m` new designs to empty
//! bins — one distinct empty bin per parameter and design — that is best
//! under the combined objective (issue #47): the saturated correlation
//! key of [`crate::space`] first (every pair's r² floored at
//! [`SELECTION_TAU`](crate::space::SELECTION_TAU)), and among exact
//! ties of the key the lower Audze–Eglais energy, evaluated on the
//! bin-center proxy (see [`AeContext`]). A pooled-r²-only comparison
//! is structurally blind inside the null-correlation manifold — the X
//! defect — so the space-filling term decides there.
//!
//! Two strategies:
//!
//! - **Exact enumeration** when the number of assignments
//!   `prod_j P(k_j, m)` does not exceed the configured cap; every
//!   assignment is scored and the best (ties resolved by the first in
//!   lexicographic order) wins.
//! - **Greedy fallback** above the cap: designs are placed one at a time,
//!   each choosing the unused bin combination that minimizes the
//!   combined objective of all designs placed so far (again
//!   deterministic, ties to the lexicographically first combination).
//!
//! The space-filling term costs one distance evaluation per
//! (candidate, context point) pair per leaf, so both strategies run it
//! under the per-chunk budget [`AE_BUDGET`]. Once the budget is spent
//! the remaining leaves compare on the saturated key alone and the
//! selection reports `ae_capped` — which voids the single-chunk-exact
//! no-op proof of the polish pass (see [`crate::augment`]).

use crate::error::{Error, Result};
use crate::scoring::{PairSeries, assignment_pair_r2s_with, assignment_score};
use crate::space;

/// Upper bound on the empty-bin cross product searched per greedy step.
const GREEDY_CROSS_CAP: u128 = 10_000_000;

/// Per-chunk budget of AE distance terms (one pairwise point distance
/// evaluation each) for the combined-objective comparison, shared by
/// the exact and the greedy path. Sized to cover the primary workflows
/// in full — a fresh explore of a few hundred designs at up to three
/// parameters stays well inside — while huge batched runs degrade
/// instead of exploding: after the budget is spent, the remaining
/// leaves compare on the saturated key alone and the chunk reports
/// `ae_capped`, which also re-enables the polish pass on an otherwise
/// provably optimal exact chunk.
const AE_BUDGET: u128 = 2_000_000_000;

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
    /// A batched run whose chunks used different strategies.
    Mixed,
}

impl SelectionStrategy {
    /// Stable lowercase name for reports: `"exact"` / `"greedy"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Greedy => "greedy",
            Self::Mixed => "mixed",
        }
    }

    /// The strategy of a batched run over its per-chunk strategies: the
    /// common strategy when every chunk agrees, mixed when they differ.
    /// An empty slice reports exact (nothing ran greedily).
    pub fn aggregate(strategies: &[Self]) -> Self {
        match strategies.split_first() {
            None => Self::Exact,
            Some((first, rest)) => {
                if rest.iter().all(|s| s == first) {
                    *first
                } else {
                    Self::Mixed
                }
            }
        }
    }
}

/// The space-filling context of a selection (issue #47): the fixed
/// points of the AE criterion and the bin-center proxy coordinates of
/// everything the search moves. Values have not materialized at
/// selection time — the new designs are represented by the normalized
/// bin centers of their candidate bins, a sound proxy because the
/// X-shaped null-correlation defect lives at bin granularity (a
/// permutation property); within-bin value draws neither cause nor
/// cure it, so they stay out of the objective.
#[derive(Debug, Clone, Copy)]
pub struct AeContext<'a> {
    /// Fixed context rows: the original designs in scope for every
    /// parameter, transformed to the unit cube. Constant for the
    /// whole run.
    pub fixed: &'a [Vec<f64>],
    /// Rows of the designs earlier chunks placed, at the normalized
    /// bin centers of their chosen bins.
    pub placed: &'a [Vec<f64>],
    /// Per parameter, the normalized bin-center coordinate of each
    /// candidate, aligned with [`BinCandidates::bins`].
    pub centers: &'a [Vec<f64>],
}

/// The chosen assignment.
#[derive(Debug, Clone)]
pub struct Selection {
    /// Per parameter: the chosen candidate index (into [`BinCandidates`]) for each new design.
    pub picks: Vec<Vec<usize>>,
    /// Score of the chosen assignment: the pooled pair r². The
    /// comparison that chose the assignment runs on the combined
    /// objective (saturated key, then AE); the pooled r² stays the
    /// reported score so the `plhs` score remains readable back from
    /// the pool statistics.
    pub score: f64,
    /// The strategy that produced this selection.
    pub strategy: SelectionStrategy,
    /// Whether the chunk's AE budget ran out mid-search, leaving part
    /// of the leaves compared on the saturated key alone — which voids
    /// the joint-optimality proof a single-chunk exact run otherwise
    /// carries.
    pub(crate) ae_capped: bool,
}

/// Selects the best assignment of `m` new designs to the empty bins
/// under the combined objective.
///
/// `candidates` holds the empty-bin candidates per parameter; `pairs` holds
/// the in-scope series used for scoring. `cap` is the maximum number of
/// assignments to enumerate exactly. `ae` supplies the space-filling
/// context of the AE tie-break.
pub fn select(
    m: usize,
    pairs: &[PairSeries],
    candidates: &[BinCandidates],
    cap: u64,
    ae: &AeContext,
) -> Result<Selection> {
    select_budgeted(m, pairs, candidates, cap, ae, AE_BUDGET)
}

/// The budgeted core of [`select`], exposed for tests: `ae_budget` is
/// the chunk's AE distance-term allowance.
pub(crate) fn select_budgeted(
    m: usize,
    pairs: &[PairSeries],
    candidates: &[BinCandidates],
    cap: u64,
    ae: &AeContext,
    ae_budget: u128,
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
    let exact = !product_exceeds(
        candidates.iter().map(|c| perm_count(c.bins.len(), m)),
        u128::from(cap),
    );
    let mut sel = if exact {
        enumerate(m, pairs, candidates, ae, ae_budget)
    } else {
        greedy(m, pairs, candidates, ae, ae_budget)
    }?;
    sel.strategy = if exact {
        SelectionStrategy::Exact
    } else {
        SelectionStrategy::Greedy
    };
    Ok(sel)
}

/// Whether the product of `factors` exceeds `threshold`, deciding exactly
/// for every threshold the callers use (the cap fits `u64`, the greedy
/// budget is 10^7; both are far below `sqrt(u128::MAX)`). The running
/// product is kept `<= threshold`, so a step multiplies two values that
/// are each `<= threshold` and cannot overflow `u128` spuriously; a failed
/// [`u128::checked_mul`] therefore proves the true product exceeds
/// `u128::MAX > threshold`. A single factor above the threshold decides
/// on its own because the running product is at least 1 (a zero factor
/// zeroes the whole product and decides immediately). This replaces the
/// old saturating product, which made cap and guard decisions correct
/// only by clamping at `u128::MAX`.
fn product_exceeds(factors: impl Iterator<Item = u128>, threshold: u128) -> bool {
    let mut product: u128 = 1;
    for factor in factors {
        if factor == 0 {
            return false;
        }
        if factor > threshold {
            return true;
        }
        product = match product.checked_mul(factor) {
            Some(p) if p <= threshold => p,
            _ => return true,
        };
    }
    false
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

/// The largest batch of new designs whose per-step bin cross product
/// `b^p` stays within the greedy budget ([`GREEDY_CROSS_CAP`]) — the
/// largest chunk the greedy fallback can search per step. For parameter
/// counts where even `2^p` exceeds the budget this degenerates to 1: the
/// standard-LHS loop of one design per chunk.
pub fn solvable_batch(p: usize) -> usize {
    let (mut lo, mut hi) = (1usize, GREEDY_CROSS_CAP as usize);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if product_exceeds(std::iter::repeat_n(mid as u128, p), GREEDY_CROSS_CAP) {
            hi = mid - 1;
        } else {
            lo = mid;
        }
    }
    lo
}

/// Whether the per-step empty-bin cross product of the given
/// per-parameter counts exceeds the greedy budget — the batch loop's
/// signal to restrict a chunk's candidate set to [`solvable_batch`]
/// bins per parameter.
pub(crate) fn empty_cross_exceeds_budget(counts: impl Iterator<Item = usize>) -> bool {
    product_exceeds(counts.map(|c| c as u128), GREEDY_CROSS_CAP)
}

// --- space-filling term -------------------------------------------------------

/// Squared Euclidean distance of two unit-cube rows.
fn dist2(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| {
            let d = x - y;
            d * d
        })
        .sum()
}

/// Sum of AE terms over all unordered pairs of `points`.
fn pair_sum(points: &[Vec<f64>]) -> f64 {
    let mut sum = 0.0;
    for a in 0..points.len() {
        for b in (a + 1)..points.len() {
            sum += space::ae_term(dist2(&points[a], &points[b]));
        }
    }
    sum
}

/// Sum of AE terms between one trial row and every context row — the
/// part of the energy that varies between greedy leaves (the energy
/// among the context rows is constant during a step and cancels in
/// every comparison).
fn cross_row_sum(rows: &[Vec<f64>], trial: &[f64]) -> f64 {
    rows.iter().map(|r| space::ae_term(dist2(trial, r))).sum()
}

/// Sum of AE terms between every trial row and every context row.
fn cross_sum(rows: &[Vec<f64>], trial: &[Vec<f64>]) -> f64 {
    trial.iter().map(|t| cross_row_sum(rows, t)).sum()
}

/// Remaining AE distance terms of one chunk's search.
struct AeBudget {
    left: u128,
    capped: bool,
}

impl AeBudget {
    fn new(budget: u128) -> Self {
        Self {
            left: budget,
            capped: false,
        }
    }

    /// Charges `terms` and reports whether they were affordable; the
    /// first unaffordable charge exhausts the budget permanently.
    fn charge(&mut self, terms: u128) -> bool {
        if self.capped {
            return false;
        }
        if terms > self.left {
            self.capped = true;
            self.left = 0;
            return false;
        }
        self.left -= terms;
        true
    }
}

/// The combined-objective comparison of one leaf against the incumbent:
/// a lower saturated key wins outright; on an exact key tie the lower
/// AE energy wins; with either AE unknown (budget exhausted) the
/// incumbent stays on a tie. Ties on both keep the incumbent, so the
/// first leaf in enumeration order wins — the historical determinism.
fn leaf_better(key: f64, ae: Option<f64>, best_key: f64, best_ae: Option<f64>) -> bool {
    match (ae, best_ae) {
        (Some(a), Some(b)) => space::saturated_better(key, a, best_key, b),
        _ => key < best_key,
    }
}

// --- exact enumeration ------------------------------------------------------

/// The incumbent of a search: the chosen picks with the pooled score
/// (reported) and the combined-objective key and AE energy used to
/// compare leaves.
struct Best {
    picks: Vec<Vec<usize>>,
    score: f64,
    key: f64,
    ae: Option<f64>,
}

struct Enumerator<'a> {
    m: usize,
    pairs: &'a [PairSeries],
    candidates: &'a [BinCandidates],
    /// Current picks: `picks[param][design]` = candidate index.
    picks: Vec<Vec<usize>>,
    /// Scratch: transformed candidate values per param/design.
    cand: Vec<Vec<f64>>,
    /// Scratch: bin-center proxy rows of the trial, `trial[design]`.
    trial: Vec<Vec<f64>>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    r2s: Vec<f64>,
    /// AE context rows: fixed originals plus earlier chunks' placements.
    rows: Vec<Vec<f64>>,
    centers: &'a [Vec<f64>],
    budget: AeBudget,
    best: Option<Best>,
}

fn enumerate(
    m: usize,
    pairs: &[PairSeries],
    candidates: &[BinCandidates],
    ae: &AeContext,
    ae_budget: u128,
) -> Result<Selection> {
    let p = candidates.len();
    let mut rows = ae.fixed.to_vec();
    rows.extend_from_slice(ae.placed);
    let mut state = Enumerator {
        m,
        pairs,
        candidates,
        picks: vec![vec![0; m]; p],
        cand: vec![vec![0.0; m]; p],
        trial: vec![vec![0.0; p]; m],
        xs: Vec::new(),
        ys: Vec::new(),
        r2s: Vec::new(),
        rows,
        centers: ae.centers,
        budget: AeBudget::new(ae_budget),
        best: None,
    };
    state.rec_param(0);
    state
        .best
        .map(|b| Selection {
            picks: b.picks,
            score: b.score,
            strategy: SelectionStrategy::Exact,
            ae_capped: state.budget.capped,
        })
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
            self.trial[slot][j] = self.centers[j][idx];
            self.rec_slot(j, slot + 1, used);
            used[idx] = false;
        }
    }

    fn score_leaf(&mut self) {
        assignment_pair_r2s_with(
            self.pairs,
            &self.cand,
            self.m,
            &mut self.xs,
            &mut self.ys,
            &mut self.r2s,
        );
        let score: f64 = self.r2s.iter().sum();
        let key = space::sat_key(&self.r2s);
        let ae = match self.best.as_ref() {
            // a strictly worse key loses without a space-filling look
            Some(b) if key > b.key => None,
            _ => {
                let terms = u128::try_from(self.rows.len() * self.m).unwrap_or(u128::MAX)
                    + u128::try_from(self.m * (self.m - 1) / 2).unwrap_or(u128::MAX);
                if self.budget.charge(terms) {
                    Some(cross_sum(&self.rows, &self.trial) + pair_sum(&self.trial))
                } else {
                    None
                }
            }
        };
        let better = match self.best.as_ref() {
            None => true,
            Some(b) => leaf_better(key, ae, b.key, b.ae),
        };
        if better {
            self.best = Some(Best {
                picks: self.picks.clone(),
                score,
                key,
                ae,
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

/// Greedy per-pair r² of placing design `design` on top of the
/// accumulated `sums`: each pair in O(1) from the running raw sums.
/// `cand[j][design]` holds the candidate value under trial.
fn incremental_pair_r2s(
    pairs: &[PairSeries],
    sums: &[PairSums],
    cand: &[Vec<f64>],
    design: usize,
    out: &mut Vec<f64>,
) {
    out.clear();
    for (pair, sums) in pairs.iter().zip(sums.iter()) {
        out.push(sums.r2_with(cand[pair.a][design], cand[pair.b][design]));
    }
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
    r2s: Vec<f64>,
    /// AE context rows: fixed originals, earlier chunks' placements and
    /// the designs this chunk already committed — every trial is
    /// compared against all of them.
    rows: Vec<Vec<f64>>,
    centers: &'a [Vec<f64>],
    /// Scratch: bin-center proxy coordinates of the current trial.
    trial: Vec<f64>,
    budget: AeBudget,
    /// The new design currently being placed.
    design: usize,
    /// The bin combination currently being tried.
    tuple: Vec<usize>,
    /// The step's incumbent: `(tuple, saturated key, AE energy)`.
    best: Option<(Vec<usize>, f64, Option<f64>)>,
}

fn greedy(
    m: usize,
    pairs: &[PairSeries],
    candidates: &[BinCandidates],
    ae: &AeContext,
    ae_budget: u128,
) -> Result<Selection> {
    if product_exceeds(
        candidates.iter().map(|c| c.bins.len() as u128),
        GREEDY_CROSS_CAP,
    ) {
        // The true cross product can exceed u128 (50 parameters x 6 empty
        // bins ~ 8.1e38), so no exact count is printed — only its order of
        // magnitude, alongside the budget the guard compares against. The
        // remedies name the real levers: the number of new designs per
        // step and the parameter count. Narrowing scopes shrinks the
        // empty-bin count only in the bin-collision case (existing
        // designs sharing bins); on a valid LHS pool the empty-bin count
        // per parameter is the number of new designs, and scopes cannot
        // reduce it.
        let magnitude: f64 = candidates
            .iter()
            .map(|c| (c.bins.len() as f64).log10())
            .sum();
        return Err(Error::Invalid(format!(
            "the greedy fallback would have to search about 10^{} bin combinations per step, \
             beyond its budget of {GREEDY_CROSS_CAP}; reduce the number of new designs per \
             step or reduce the number of parameters, and narrow the scopes only when the \
             existing designs share bins",
            magnitude.round() as u64
        )));
    }
    let p = candidates.len();
    let mut rows = ae.fixed.to_vec();
    rows.extend_from_slice(ae.placed);
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
        r2s: Vec::new(),
        rows,
        centers: ae.centers,
        trial: vec![0.0; p],
        budget: AeBudget::new(ae_budget),
        design: 0,
        tuple: vec![0; p],
        best: None,
    };
    for i in 0..m {
        state.design = i;
        state.best = None;
        state.rec(0);
        let (chosen, _, _) = state
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
        ae_capped: state.budget.capped,
    })
}

impl Greedy<'_> {
    fn rec(&mut self, j: usize) {
        if j == self.candidates.len() {
            for jj in 0..j {
                self.cand[jj][self.design] = self.candidates[jj].t[self.tuple[jj]];
                self.trial[jj] = self.centers[jj][self.tuple[jj]];
            }
            let placed = self.design + 1;
            if placed <= REFERENCE_SCORING_PLACED_LIMIT {
                assignment_pair_r2s_with(
                    self.pairs,
                    &self.cand,
                    placed,
                    &mut self.xs,
                    &mut self.ys,
                    &mut self.r2s,
                );
            } else {
                incremental_pair_r2s(
                    self.pairs,
                    &self.sums,
                    &self.cand,
                    self.design,
                    &mut self.r2s,
                );
            }
            let key = space::sat_key(&self.r2s);
            let ae = match self.best.as_ref() {
                // a strictly worse key loses without a space-filling look
                Some((_, best_key, _)) if key > *best_key => None,
                _ => {
                    let terms = u128::try_from(self.rows.len()).unwrap_or(u128::MAX);
                    if self.budget.charge(terms) {
                        Some(cross_row_sum(&self.rows, &self.trial))
                    } else {
                        None
                    }
                }
            };
            let better = match self.best.as_ref() {
                None => true,
                Some((_, best_key, best_ae)) => leaf_better(key, ae, *best_key, *best_ae),
            };
            if better {
                self.best = Some((self.tuple.clone(), key, ae));
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
        // the placed design joins the space-filling context at its
        // bin-center proxy position
        let row: Vec<f64> = (0..self.centers.len())
            .map(|j| self.centers[j][chosen[j]])
            .collect();
        self.rows.push(row);
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
        let ae = ae_of(&cands);
        let sel = select(2, &pairs, &cands, 1_000_000, &ae).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Exact);
        assert_eq!(sel.strategy.name(), "exact");

        // the same setup with cap 0 always exceeds the cap: greedy runs
        let sel = select(2, &pairs, &cands, 0, &ae).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Greedy);
        assert_eq!(sel.strategy.name(), "greedy");
    }

    #[test]
    fn exact_selection_enforces_distinct_bins() {
        // 2 params, 3 empty bins each, m = 2
        let cands = candidates(&[(vec![1, 4, 7], 0.0), (vec![2, 5, 8], 0.0)]);
        let pairs = vec![pair_series(0, 1, vec![0.1, 0.4, 0.9], vec![0.9, 0.6, 0.1])];
        let ae = ae_of(&cands);
        let sel = select(2, &pairs, &cands, 1_000_000, &ae).unwrap();
        for j in 0..2 {
            assert_ne!(sel.picks[j][0], sel.picks[j][1]);
        }
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
        let ae = ae_of(&cands);
        let sel = select(2, &pairs, &cands, 0, &ae).unwrap();
        for j in 0..2 {
            assert_ne!(sel.picks[j][0], sel.picks[j][1]);
        }
    }

    #[test]
    fn rejects_fewer_empty_bins_than_designs() {
        let cands = candidates(&[(vec![3], 0.0)]);
        let pairs: Vec<PairSeries> = vec![];
        let ae = ae_of(&cands);
        assert!(select(2, &pairs, &cands, 1_000_000, &ae).is_err());
    }

    #[test]
    fn perm_count_matches_formula() {
        assert_eq!(perm_count(5, 2), 20);
        assert_eq!(perm_count(3, 3), 6);
        assert_eq!(perm_count(2, 5), 0);
    }

    #[test]
    fn product_exceeds_is_exact_at_the_threshold() {
        // equal to the threshold is not "exceeds"
        assert!(!product_exceeds([100u128, 10].into_iter(), 1_000));
        // one over the threshold exceeds
        assert!(product_exceeds([100u128, 11].into_iter(), 1_000));
        // an empty product is 1, never exceeding
        assert!(!product_exceeds(std::iter::empty(), 1_000_000));
        // a zero factor makes the whole product zero
        assert!(!product_exceeds([5u128, 0, u128::MAX].into_iter(), 100));
    }

    #[test]
    fn product_exceeds_needs_no_saturation() {
        // 6^60 > u128::MAX: the running product exceeds the threshold after
        // eight factors (6^8 = 1_679_616 > 1_000_000), so the predicate
        // decides without ever saturating
        assert!(product_exceeds(std::iter::repeat_n(6u128, 60), 1_000_000));
        // a saturated factor (u128::MAX) exceeds any reachable threshold on
        // its own, and the true product is even larger
        assert!(product_exceeds([u128::MAX, 2, 3].into_iter(), 1_000_000));
        // 40 empty bins x 40 empty bins: the true cross product is 1600
        assert!(!product_exceeds([40u128, 40].into_iter(), 1_000_000));
        // the fresh-pool guard case: 50 parameters x 6 empty bins, the
        // true product 6^50 ~ 8.1e38 even overflows u128
        assert!(product_exceeds(std::iter::repeat_n(6u128, 50), 1_000_000));
    }

    #[test]
    fn solvable_batch_is_the_largest_batch_within_the_budget() {
        // b^p <= GREEDY_CROSS_CAP (10_000_000)
        assert_eq!(solvable_batch(1), 10_000_000, "10_000_000^1 = budget");
        // 3162^2 = 9_998_244 <= budget, 3163^2 = 10_004_569 > budget
        assert_eq!(solvable_batch(2), 3162);
        // 7^8 = 5_764_801 <= budget, 8^8 = 16_777_216 > budget
        assert_eq!(solvable_batch(8), 7);
        // 50 parameters: 2^50 already exceeds the budget, so the batch
        // degenerates to the standard-LHS loop of one design per chunk
        assert_eq!(solvable_batch(50), 1);
        // a huge parameter count stays at 1
        assert_eq!(solvable_batch(500), 1);
    }

    #[test]
    fn strategy_aggregate_reports_mixed_when_strategies_differ() {
        use SelectionStrategy::{Exact, Greedy, Mixed};
        assert_eq!(SelectionStrategy::aggregate(&[Exact, Exact]), Exact);
        assert_eq!(
            SelectionStrategy::aggregate(&[Greedy, Greedy, Greedy]),
            Greedy
        );
        assert_eq!(SelectionStrategy::aggregate(&[Exact]), Exact);
        assert_eq!(SelectionStrategy::aggregate(&[Greedy, Exact]), Mixed);
        assert_eq!(SelectionStrategy::aggregate(&[Exact, Exact, Greedy]), Mixed);
        assert_eq!(Mixed.name(), "mixed");
    }

    #[test]
    fn greedy_guard_reports_a_magnitude_not_a_saturated_count() {
        // 50 parameters x 6 empty bins each (a fresh pool explored with
        // m = 6): the cross product 6^50 ~ 8.1e38 overflows u128, the old
        // message printed the saturated count 340282366920938463463374607431768211455
        // as if it were exact
        let bins: Vec<usize> = (0..6).collect();
        let cands = candidates(&(0..50).map(|_| (bins.clone(), 0.0)).collect::<Vec<_>>());
        let pairs: Vec<PairSeries> = vec![];
        let err = select(6, &pairs, &cands, 0, &ae_of(&cands)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("10^39"),
            "message should report the magnitude: {msg}"
        );
        assert!(
            msg.contains("10000000"),
            "message should state the search budget: {msg}"
        );
        assert!(
            !msg.contains("340282366920938463463374607431768211455"),
            "message must not print the saturated u128 count as if exact: {msg}"
        );
        assert!(
            msg.contains("new designs"),
            "message should name the remedy of fewer new designs: {msg}"
        );
        assert!(
            msg.contains("reduce the number of parameters"),
            "message should name the parameter count as the real lever: {msg}"
        );
        assert!(
            msg.contains("share bins"),
            "message should scope the narrow-scopes advice to the bin-collision case: {msg}"
        );
    }

    // --- combined objective (issue #47) --------------------------------

    /// Bin-center proxy coordinates for the tests: `(b + 0.5) / n` on
    /// the unit cube.
    fn centers_grid(bins_per_param: &[usize]) -> Vec<Vec<f64>> {
        bins_per_param
            .iter()
            .map(|&n| (0..n).map(|b| (b as f64 + 0.5) / n as f64).collect())
            .collect()
    }

    /// A neutral AE context over the given candidates: each candidate's
    /// drawn value doubles as its proxy coordinate. The owned centers
    /// are leaked into the test process — `AeContext` borrows them and
    /// each test allocates a few dozen floats.
    fn ae_of(cands: &[BinCandidates]) -> AeContext<'static> {
        let centers: Vec<Vec<f64>> = cands.iter().map(|c| c.t.clone()).collect();
        AeContext {
            fixed: &[],
            placed: &[],
            centers: Box::leak(centers.into_boxed_slice()),
        }
    }

    #[test]
    fn exact_breaks_null_score_ties_by_space_filling() {
        // m = 1 on an empty pair series: every one of the 16 bin tuples
        // has a single-sample series, so every r^2 is exactly 0.0 — the
        // pure-r^2 comparator cannot tell the tuples apart and keeps
        // the lexicographically first. The combined objective must
        // instead spread the one new point away from the fixed context
        // point at (0.5, 0.2): the unique lowest-AE tuples are the
        // (bin 0, bin 3) and (bin 3, bin 3) corners (tied by symmetry),
        // and the lexicographically first of those wins.
        let cands = candidates(&[(vec![0, 1, 2, 3], 0.0), (vec![0, 1, 2, 3], 0.0)]);
        let pairs = vec![pair_series(0, 1, Vec::new(), Vec::new())];
        let centers = centers_grid(&[4, 4]);
        let fixed = vec![vec![0.5, 0.2]];
        let ae = AeContext {
            fixed: &fixed,
            placed: &[],
            centers: &centers,
        };
        let sel = select(1, &pairs, &cands, 1_000_000, &ae).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Exact);
        assert_eq!(
            (sel.picks[0][0], sel.picks[1][0]),
            (0, 3),
            "exact must break the null-score tie by space-filling"
        );
        assert!(!sel.ae_capped);
    }

    #[test]
    fn greedy_breaks_null_score_ties_by_space_filling() {
        // the same tie under the greedy fallback (cap 0 forces it)
        let cands = candidates(&[(vec![0, 1, 2, 3], 0.0), (vec![0, 1, 2, 3], 0.0)]);
        let pairs = vec![pair_series(0, 1, Vec::new(), Vec::new())];
        let centers = centers_grid(&[4, 4]);
        let fixed = vec![vec![0.5, 0.2]];
        let ae = AeContext {
            fixed: &fixed,
            placed: &[],
            centers: &centers,
        };
        let sel = select(1, &pairs, &cands, 0, &ae).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Greedy);
        assert_eq!(
            (sel.picks[0][0], sel.picks[1][0]),
            (0, 3),
            "greedy must break the null-score tie by space-filling"
        );
        assert!(!sel.ae_capped);
    }

    #[test]
    fn ae_budget_caps_the_comparison_and_reports_it() {
        // the same null-tie setup: with no AE budget at all, the
        // comparison degrades to the saturated key alone — every key
        // ties, so the lexicographically first tuple wins (the
        // pre-combined-objective behaviour) — and the chunk reports
        // that it was capped
        let cands = candidates(&[(vec![0, 1, 2, 3], 0.0), (vec![0, 1, 2, 3], 0.0)]);
        let pairs = vec![pair_series(0, 1, Vec::new(), Vec::new())];
        let centers = centers_grid(&[4, 4]);
        let fixed = vec![vec![0.5, 0.2]];
        let ae = AeContext {
            fixed: &fixed,
            placed: &[],
            centers: &centers,
        };
        let exact = select_budgeted(1, &pairs, &cands, 1_000_000, &ae, 0).unwrap();
        assert_eq!((exact.picks[0][0], exact.picks[1][0]), (0, 0));
        assert!(exact.ae_capped, "the exhausted budget must be reported");
        let greedy = select_budgeted(1, &pairs, &cands, 0, &ae, 0).unwrap();
        assert_eq!((greedy.picks[0][0], greedy.picks[1][0]), (0, 0));
        assert!(greedy.ae_capped);
        // a generous budget restores the space-filling tie-break
        let funded = select_budgeted(1, &pairs, &cands, 1_000_000, &ae, 1_000_000).unwrap();
        assert_eq!((funded.picks[0][0], funded.picks[1][0]), (0, 3));
        assert!(!funded.ae_capped);
    }

    #[test]
    fn exact_prefers_spread_assignments_on_a_fresh_pool() {
        // 2 parameters, 3 empty bins, m = 3, no base series: all 36
        // assignments are exactly r^2-null (single-sample pairs), so
        // the AE tie-break alone decides. The lexicographically first
        // assignment is the main diagonal (all three points on
        // y = x, the worst spread); the combined objective must pick
        // one with strictly lower AE.
        let cands = candidates(&[(vec![0, 1, 2], 0.0), (vec![0, 1, 2], 0.0)]);
        let pairs = vec![pair_series(0, 1, Vec::new(), Vec::new())];
        let centers = centers_grid(&[3, 3]);
        let ae = AeContext {
            fixed: &[],
            placed: &[],
            centers: &centers,
        };
        let sel = select(3, &pairs, &cands, 1_000_000, &ae).unwrap();
        assert_eq!(sel.strategy, SelectionStrategy::Exact);
        let diagonal: Vec<Vec<f64>> = (0..3).map(|i| centers[0][i..i + 1].to_vec()).collect();
        let diagonal_energy = crate::space::ae_energy(&diagonal);
        let chosen: Vec<Vec<f64>> = (0..3)
            .map(|i| vec![centers[0][sel.picks[0][i]], centers[1][sel.picks[1][i]]])
            .collect();
        let chosen_energy = crate::space::ae_energy(&chosen);
        assert!(
            chosen_energy < diagonal_energy - 0.1,
            "chosen AE {chosen_energy} must beat the diagonal {diagonal_energy}"
        );
    }

    #[test]
    fn greedy_scores_within_the_tau_headroom_of_exact() {
        // exact minimizes the saturated key over every assignment, and
        // a key never exceeds the pooled score by more than one floor
        // per pair — so exact's pooled score cannot exceed greedy's by
        // more than pairs * SELECTION_TAU, whichever way the AE
        // tie-break trades within the floor
        let cands = candidates(&[(vec![1, 4, 7], 0.0), (vec![2, 5, 8], 0.05)]);
        let pairs = vec![pair_series(
            0,
            1,
            vec![0.1, 0.4, 0.55, 0.9],
            vec![0.9, 0.6, 0.45, 0.1],
        )];
        let ae = ae_of(&cands);
        let exact = select(2, &pairs, &cands, 1_000_000, &ae).unwrap();
        let greedy_sel = select(2, &pairs, &cands, 1, &ae).unwrap();
        assert!(
            exact.score <= greedy_sel.score + crate::space::SELECTION_TAU + 1e-9,
            "exact {} vs greedy {}",
            exact.score,
            greedy_sel.score
        );
    }

    #[test]
    fn incremental_pair_r2s_match_the_reference_scorer() {
        // a pair with a non-empty base series plus 30 placed designs: the
        // O(1) incremental per-pair scorer must track the two-pass reference
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
        let (mut rx, mut ry, mut rr) = (Vec::new(), Vec::new(), Vec::new());
        let mut incremental = Vec::new();
        for i in 0..30 {
            incremental_pair_r2s(&pairs, &sums, &cand, i, &mut incremental);
            assignment_pair_r2s_with(&pairs, &cand, i + 1, &mut rx, &mut ry, &mut rr);
            assert_eq!(incremental.len(), rr.len());
            for (a, b) in incremental.iter().zip(rr.iter()) {
                assert!(
                    (a - b).abs() < 1e-9,
                    "design {i}: incremental={a} reference={b}"
                );
            }
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
        let sel = select(15, &pairs, &cands, 0, &ae_of(&cands)).unwrap();
        for j in 0..2 {
            let mut bins: Vec<usize> = sel.picks[j].clone();
            bins.sort_unstable();
            bins.dedup();
            assert_eq!(bins.len(), 15, "parameter {j}: designs share bins");
        }
        assert!(sel.score.is_finite() && sel.score >= 0.0);
    }
}
