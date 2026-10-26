//! Pooled Latin Hypercube Sampling (`plhs`).
//!
//! `plhs` augments a pool of existing experiment designs with new designs
//! that preserve the Latin Hypercube property inside a per-parameter scope:
//!
//! 1. Each parameter has a definition range `[min, max]` and a distribution
//!    (default `uniform`, optionally `gaussian` truncated to the range).
//!    The scope narrows that range; a missing scope falls back to the
//!    parameter definition.
//! 2. Per parameter, the scope is mapped to `[0, 1]` through the truncated
//!    CDF; existing in-scope samples are located in `n + m` equal-mass bins
//!    (`n` = in-scope samples, `m` = new designs to add).
//! 3. At least `m` bins are empty (pigeonhole); every empty bin receives a
//!    random candidate value drawn from its probability mass.
//! 4. Assignments of the `m` new designs to distinct empty bins per
//!    parameter are scored by the sum of squared Pearson correlations over
//!    all parameter pairs (in-scope samples plus the candidates) and the
//!    best assignment wins. Exact enumeration up to a configurable cap,
//!    greedy sequential selection beyond it. Large runs are placed in
//!    chunks sized so each chunk's search fits the greedy budget; at 50+
//!    parameters that degenerates to the standard-LHS loop of one design
//!    per chunk. After the chunk loop a deterministic swap descent (the
//!    polish pass) refines the finished assignment; see
//!    [`polish`](mod@crate::polish).
//!
//! All computation happens on the transformed `[0, 1]` values; real values
//! are produced only for output.
//!
//! # Explore mode
//!
//! [`explore`] is the scoped augmentation's configuration-free
//! counterpart: every parameter is treated as uniform over its full
//! definition range `[min, max]`, ignoring scopes and non-uniform
//! distributions, so the new designs cover the whole surface instead of a
//! specific section. All existing designs lie in the definition range by
//! definition, so the entire pool takes part in the binning. Explore the
//! whole surface first, then refine sections with [`augment`] and a scope.
//!
//! # Determinism
//!
//! Candidate values are drawn from a ChaCha8 RNG seeded with an explicit
//! `seed` (the binary defaults to a fixed seed). The same input and seed
//! always produce the same output.
//!
//! # Example
//!
//! ```
//! use plhs::{augment, Design, Parameter};
//!
//! let params = vec![
//!     Parameter::uniform("x", 0.0, 10.0)?.with_scope(2.0, 8.0)?,
//! ];
//! let designs = vec![
//!     Design { name: "0.1".into(), values: vec![2.5] },
//!     Design { name: "0.2".into(), values: vec![4.0] },
//!     Design { name: "0.3".into(), values: vec![7.0] },
//! ];
//! let aug = augment(&params, &designs, 2, 42, 1_000_000, true)?;
//! assert_eq!(aug.new_designs.len(), 2);
//! // existing names use iteration 0, so the new ones are "1.1" and "1.2"
//! assert_eq!(aug.new_designs[0].name, "1.1");
//! assert_eq!(aug.new_designs[1].name, "1.2");
//! # Ok::<(), plhs::Error>(())
//! ```
//!
//! Exploring the whole surface instead:
//!
//! ```
//! use plhs::{explore, Design, Parameter};
//!
//! // scope and gaussian distribution are ignored in explore mode
//! let params = vec![
//!     Parameter::gaussian("x", 0.0, 10.0, 5.0, 2.0)?.with_scope(2.0, 8.0)?,
//! ];
//! let designs = vec![
//!     Design { name: "0.1".into(), values: vec![2.5] },
//!     Design { name: "0.2".into(), values: vec![4.0] },
//!     Design { name: "0.3".into(), values: vec![7.0] },
//! ];
//! let aug = explore(&params, &designs, 2, 42, 1_000_000, true)?;
//! // the new designs cover the full definition range [0.0, 10.0]
//! for design in &aug.new_designs {
//!     assert!((0.0..=10.0).contains(&design.values[0]));
//! }
//! # Ok::<(), plhs::Error>(())
//! ```

pub mod binning;
pub mod polish;
pub mod scoring;
pub mod selection;

pub use doe_types::error;
pub use doe_types::scaling;
pub use doe_types::{Design, Distribution, Error, Parameter, Result};
pub use polish::PolishReport;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use binning::{bin_count, candidate_t, empty_bins, occupied_bins, sample_bins};
use scoring::{PairSeries, assignment_score};
pub use selection::SelectionStrategy;
use selection::{BinCandidates, empty_cross_exceeds_budget, select, solvable_batch};

/// The result of [`augment`].
#[derive(Debug, Clone)]
pub struct Augmentation {
    /// The `m` new designs, in design order `1..=m`.
    pub new_designs: Vec<Design>,
    /// Score of the chosen assignment: sum of squared pair correlations.
    pub score: f64,
    /// Iteration number detected from the existing design names.
    pub iteration: u64,
    /// The selection strategy that chose the assignment.
    pub strategy: SelectionStrategy,
    /// The number of chunks the `m` new designs were placed in (1 when
    /// the whole run fit a single selection).
    pub chunks: usize,
    /// Report of the post-placement refinement pass, when it ran: the
    /// swap descent over the finished assignment (see
    /// [`polish`](mod@crate::polish)). `None` when the pass was
    /// disabled or skipped — a single-chunk exact run is already
    /// jointly optimal over the pass's swap space, so polishing it is
    /// a provable no-op.
    pub polish: Option<PolishReport>,
}

/// Samples `m` new designs for the given parameters, extending the Latin
/// Hypercube inside each parameter's scope.
///
/// `cap` is the maximum number of assignments enumerated exactly before
/// falling back to greedy sequential selection. `polish` runs the
/// deterministic post-placement refinement pass (see
/// [`polish`](mod@crate::polish)) unless the run is a single-chunk exact
/// selection, which the pass provably cannot improve.
///
/// # Batching
///
/// The `m` new designs are placed in chunks of at most
/// [`solvable_batch`](selection::solvable_batch)`(p)` designs, the largest
/// batch whose per-step bin cross product `b^p` fits the greedy fallback's
/// budget; at 50+ parameters that degenerates to `b = 1` — the standard-LHS
/// loop. Every chunk draws its candidates from the empty bins of the final
/// `n + m` bin partition, re-evaluated against the designs earlier chunks
/// placed, selects, and appends — so the finished pool keeps the Latin
/// Hypercube property of one design per bin, and a run that fits a single
/// chunk takes the historical single-shot path, draw for draw. A single
/// chunk reports the selection's score and strategy; a batched run
/// reports the pooled score over all pairs with all new designs and the
/// aggregate strategy (`mixed` when the chunks differed).
///
/// # Polish
///
/// Chunks are optimized one at a time, so the finished assignment is
/// improvable even when every chunk ran exact: greedy chunks drift, and
/// batched exact chunks are per-chunk optimal, never jointly. With
/// `polish` enabled, a deterministic swap descent refines the finished
/// assignment after the chunk loop, before any values materialize; its
/// report is returned in [`Augmentation::polish`]. A single-chunk exact
/// run skips the pass — exact already searched the whole assignment
/// space, of which the descent's swap space is a subset.
///
/// # Errors
///
/// Returns an [`Error`] for any validation problem: duplicate parameters,
/// invalid scope entries, designs missing parameter values, parameter pairs
/// with one or two shared in-scope designs (a pair with no shared in-scope
/// designs, e.g. an empty pool, is valid — the new designs alone carry its
/// pooled correlation), and violated invariants.
pub fn augment(
    params: &[Parameter],
    designs: &[Design],
    m: usize,
    seed: u64,
    cap: u64,
    polish: bool,
) -> Result<Augmentation> {
    if params.is_empty() {
        return Err(Error::Invalid(
            "at least one parameter is required".to_owned(),
        ));
    }
    if m == 0 {
        return Err(Error::Invalid(
            "the number of new designs must be at least 1".to_owned(),
        ));
    }
    let p = params.len();
    for design in designs {
        if design.values.len() != p {
            return Err(Error::Design {
                name: design.name.clone(),
                reason: format!(
                    "has {} value(s), expected one value per parameter ({p})",
                    design.values.len()
                ),
            });
        }
    }

    // Per-parameter in-scope sets and transformed values (scope judgement is
    // per parameter, so the counts may differ between parameters).
    let mut in_scope: Vec<Vec<usize>> = Vec::with_capacity(p);
    let mut t_values: Vec<Vec<f64>> = Vec::with_capacity(p);
    for (j, param) in params.iter().enumerate() {
        let mut idx = Vec::new();
        let mut t = Vec::new();
        for (d, design) in designs.iter().enumerate() {
            let value = design.values[j];
            if !value.is_finite() {
                return Err(Error::Design {
                    name: design.name.clone(),
                    reason: format!("value for parameter '{}' must be finite", param.name),
                });
            }
            if param.in_scope(value) {
                idx.push(d);
                t.push(param.transform(value));
            }
        }
        in_scope.push(idx);
        t_values.push(t);
    }

    // Batched selection (issue #22): the m new designs are placed in
    // chunks of at most `batch` designs, the largest batch whose
    // per-step bin cross product b^p fits the greedy budget. Every
    // chunk draws candidates from the empty bins of the final n + m
    // partition — re-evaluated against the designs placed so far — so
    // the finished pool keeps one design per bin per parameter. With a
    // single chunk this is the historical single-shot path, draw for
    // draw: the restriction below fires exactly when the old greedy
    // guard would have, so every historically successful run keeps its
    // full candidate set.
    let solvable = solvable_batch(p);
    let batch = solvable.min(m);
    let chunks = m.div_ceil(batch);

    let mut seed_bytes = [0u8; 32];
    seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let mut rng = ChaCha8Rng::from_seed(seed_bytes);

    // Designs placed by earlier chunks: real values for the output,
    // scope-transformed values for occupancy and scoring.
    let mut placed_values: Vec<Vec<f64>> = vec![Vec::new(); p];
    let mut placed_t: Vec<Vec<f64>> = vec![Vec::new(); p];
    let mut strategies: Vec<SelectionStrategy> = Vec::with_capacity(chunks);
    let mut last_chunk_score = 0.0;
    // Base pair series of the ORIGINAL designs, built and validated in
    // the first chunk — after its candidates, keeping the historical
    // error precedence (pigeonhole invariant before pair validation).
    // Validating only the originals matters: the pair gate rejects 1-2
    // shared in-scope designs, and on an empty pool the first chunk
    // would otherwise fault on the very designs it just placed.
    let mut base_pairs: Vec<PairSeries> = Vec::new();

    for (chunk, start) in (0..m).step_by(batch).enumerate() {
        let b = batch.min(m - start);

        // Empty bins of the final partition per parameter, against the
        // original designs and everything placed so far.
        let mut empties: Vec<Vec<usize>> = Vec::with_capacity(p);
        for (j, param) in params.iter().enumerate() {
            let bins = bin_count(in_scope[j].len(), m);
            let mut ts = t_values[j].clone();
            ts.extend_from_slice(&placed_t[j]);
            let occupied = occupied_bins(&ts, bins);
            let empty = empty_bins(bins, &occupied);
            if empty.len() < b {
                return Err(Error::Invariant(format!(
                    "parameter '{}': {} empty bin(s) for {b} new design(s); \
                     the pigeonhole invariant (>= b empty bins) is violated",
                    param.name,
                    empty.len()
                )));
            }
            empties.push(empty);
        }

        // Restrict the candidate set when the empty-bin cross product
        // exceeds the greedy budget: sample `solvable` empty bins per
        // parameter, which bounds the per-step cross product by
        // `solvable^p` — inside the budget by construction.
        let restrict = empty_cross_exceeds_budget(empties.iter().map(|e| e.len()));

        let mut candidates: Vec<BinCandidates> = Vec::with_capacity(p);
        for (j, param) in params.iter().enumerate() {
            let bins = bin_count(in_scope[j].len(), m);
            let empty = if restrict {
                sample_bins(&mut rng, &empties[j], solvable.min(empties[j].len()))
            } else {
                empties[j].clone()
            };
            let mut t = Vec::with_capacity(empty.len());
            let mut values = Vec::with_capacity(empty.len());
            for &bin in &empty {
                let ct = candidate_t(&mut rng, bin, bins);
                t.push(ct);
                values.push(param.inverse_transform(ct));
            }
            candidates.push(BinCandidates {
                bins: empty,
                t,
                values,
            });
        }

        // Base pair series of the original designs (first chunk only:
        // after the candidates above, preserving the error order).
        if chunk == 0 {
            base_pairs = base_pairs_of(params, designs)?;
        }

        // Working pairs: the base series plus every design placed so far
        // — placed designs are in scope for all parameters by
        // construction.
        let mut working: Vec<PairSeries> = base_pairs.clone();
        for pair in &mut working {
            pair.xs.extend_from_slice(&placed_t[pair.a]);
            pair.ys.extend_from_slice(&placed_t[pair.b]);
        }

        // Selection of the chunk.
        let selection = select(b, &working, &candidates, cap)?;
        strategies.push(selection.strategy);
        last_chunk_score = selection.score;

        // Append the chunk's designs to the placed pool.
        for i in 0..b {
            for j in 0..p {
                let pick = selection.picks[j][i];
                placed_values[j].push(candidates[j].values[pick]);
                placed_t[j].push(candidates[j].t[pick]);
            }
        }
    }

    // Post-placement refinement (issue #24): chunks are optimized one at
    // a time, so the finished assignment is improvable — greedy chunks
    // drift, batched exact chunks are per-chunk optimal, never jointly.
    // A single-chunk exact run is the exception: exact searched the whole
    // assignment space, of which the descent's swap space is a subset,
    // so polishing it is a provable no-op and is skipped to keep those
    // outputs byte-exact.
    let strategy = SelectionStrategy::aggregate(&strategies);
    let polish_report = if polish && !(chunks == 1 && strategy == SelectionStrategy::Exact) {
        Some(polish::polish(
            &base_pairs,
            &mut placed_t,
            &mut placed_values,
        ))
    } else {
        None
    };

    // New designs and names (from the original names only, as before).
    let existing_names: Vec<&str> = designs.iter().map(|d| d.name.as_str()).collect();
    let mut iteration = next_iteration(existing_names.iter().copied());
    let existing: BTreeSet<String> = designs.iter().map(|d| d.name.clone()).collect();
    loop {
        let collides = (0..m).any(|i| existing.contains(&format!("{iteration}.{}", i + 1)));
        if !collides {
            break;
        }
        iteration += 1;
    }
    let new_designs = (0..m)
        .map(|i| {
            let values: Vec<f64> = (0..p).map(|j| placed_values[j][i]).collect();
            Design {
                name: format!("{iteration}.{}", i + 1),
                values,
            }
        })
        .collect();

    let score = match &polish_report {
        // a polished run reports the score after the pass
        Some(report) => report.score_after,
        None if chunks == 1 => last_chunk_score,
        // Pooled final score: every base pair with all m placed designs.
        None => assignment_score(&base_pairs, &placed_t, m),
    };

    Ok(Augmentation {
        new_designs,
        score,
        iteration,
        strategy,
        chunks,
        polish: polish_report,
    })
}

/// Pair series for scoring: per pair, the ORIGINAL designs in scope for
/// both parameters (scope-transformed values), with the shared-design
/// validation of [`augment`]. The designs a batched run places itself are
/// appended per chunk by the caller and never pass this gate.
fn base_pairs_of(params: &[Parameter], designs: &[Design]) -> Result<Vec<PairSeries>> {
    let p = params.len();
    let mut pairs = Vec::with_capacity(p * (p - 1) / 2);
    for a in 0..p {
        for b in (a + 1)..p {
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            for design in designs {
                let (va, vb) = (design.values[a], design.values[b]);
                if params[a].in_scope(va) && params[b].in_scope(vb) {
                    xs.push(params[a].transform(va));
                    ys.push(params[b].transform(vb));
                }
            }
            // A pair with no shared in-scope designs (e.g. an empty pool
            // straight from `sampling init`) is valid: its pooled
            // correlation is then carried by the new designs alone. A
            // pair with one or two shared designs has a degenerate
            // existing correlation and is rejected.
            if !xs.is_empty() && xs.len() < 3 {
                return Err(Error::Pair {
                    a: params[a].name.clone(),
                    b: params[b].name.clone(),
                    reason: format!(
                        "only {} in-scope design(s) shared, at least 3 are required",
                        xs.len()
                    ),
                });
            }
            pairs.push(PairSeries { a, b, xs, ys });
        }
    }
    Ok(pairs)
}

use std::collections::BTreeSet;

/// Detects the next iteration number from design names of the form
/// `{iteration}.{design}` (e.g. `12.3`): the maximum iteration found plus
/// one. Names that do not match the pattern are ignored; when no name
/// matches, iteration 1 is used.
pub fn next_iteration<'a>(names: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut max: u64 = 0;
    let mut found = false;
    for name in names {
        let mut parts = name.split('.');
        if let (Some(it), Some(design), None) = (parts.next(), parts.next(), parts.next()) {
            if let (Ok(it), Ok(_design)) = (it.parse::<u64>(), design.parse::<u64>()) {
                found = true;
                max = max.max(it);
            }
        }
    }
    if found { max + 1 } else { 1 }
}

/// Samples `m` new designs that explore the whole surface.
///
/// The scoped refinement counterpart of [`augment`] without any
/// configuration: every parameter is treated as uniform over its full
/// definition range `[min, max]`, so the new designs cover the entire
/// surface — tails, corners and sections outside the scope included —
/// instead of a specific section. Scopes and non-uniform distributions of
/// `params` are ignored; the new designs still fill empty equal-width
/// bins of the full range (all existing designs lie inside the
/// definition range by definition, so the whole pool takes part in the
/// pooled Latin Hypercube property) and the assignment minimizes the same
/// pair-correlation score.
///
/// `cap` and `seed` have the same meaning as in [`augment`]; `polish`
/// enables the post-placement refinement pass under the same
/// applicability rule (never on a single-chunk exact run).
///
/// # Errors
///
/// Returns an [`Error`] under the same conditions as [`augment`].
pub fn explore(
    params: &[Parameter],
    designs: &[Design],
    m: usize,
    seed: u64,
    cap: u64,
    polish: bool,
) -> Result<Augmentation> {
    let full: Vec<Parameter> = params.iter().map(Parameter::full_surface).collect();
    augment(&full, designs, m, seed, cap, polish)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_reports_for_greedy_runs_and_never_worsens() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
            Parameter::uniform("y", 0.0, 10.0)
                .unwrap()
                .with_scope(3.0, 7.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = (0..8)
            .map(|i| Design {
                name: format!("0.{}", i + 1),
                values: vec![2.2 + i as f64 * 0.5, 3.1 + (i % 5) as f64 * 0.8],
            })
            .collect();
        // cap 0 forces greedy: the run is polished by default
        let plain = augment(&params, &designs, 2, 7, 0, false).unwrap();
        let polished = augment(&params, &designs, 2, 7, 0, true).unwrap();
        assert!(plain.polish.is_none(), "disabled polish reports None");
        let report = polished
            .polish
            .as_ref()
            .expect("greedy runs are polished by default");
        assert_eq!(polished.strategy, SelectionStrategy::Greedy);
        // the descent is monotone: the score after is never worse
        assert!(
            report.score_after <= report.score_before + 1e-12,
            "polish worsened the score: {} -> {}",
            report.score_before,
            report.score_after
        );
        assert!(polished.score <= plain.score + 1e-12);
        // the reported score is the post-polish score
        assert_eq!(report.score_after, polished.score);
    }

    #[test]
    fn polish_is_skipped_for_single_chunk_exact_runs() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
            Parameter::gaussian("y", 0.0, 10.0, 5.0, 2.0)
                .unwrap()
                .with_scope(3.0, 7.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = (0..12)
            .map(|i| Design {
                name: format!("0.{}", i + 1),
                values: vec![2.2 + i as f64 * 0.5, 3.1 + (i % 5) as f64 * 0.8],
            })
            .collect();
        // the exact enumeration already searched the whole assignment
        // space, of which the polish swap space is a subset: the pass is
        // a provable no-op and stays skipped (byte-exact outputs)
        let aug = augment(&params, &designs, 3, 7, 1_000_000, true).unwrap();
        assert_eq!(aug.strategy, SelectionStrategy::Exact);
        assert_eq!(aug.chunks, 1);
        assert!(aug.polish.is_none(), "exact single chunk is not polished");
        let plain = augment(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs, plain.new_designs);
        assert_eq!(aug.score, plain.score);
    }

    #[test]
    fn polish_runs_on_batched_runs_and_keeps_the_lhs_property() {
        // 15 parameters batch to chunks of 2: three designs = two
        // chunks. The empty-bin cross product 3^15 exceeds the greedy
        // budget, so the candidate set is restricted to 2 bins per
        // parameter — P(2, 2)^15 = 32768 fits the cap and both chunks
        // run exact. That makes this a batched-exact run: per-chunk
        // optimal, never jointly, so the polish pass applies to it.
        let params: Vec<Parameter> = (0..15)
            .map(|j| Parameter::uniform(format!("p{j}"), 0.0, 10.0).unwrap())
            .collect();
        let plain = explore(&params, &[], 3, 1, 1_000_000, false).unwrap();
        let polished = explore(&params, &[], 3, 1, 1_000_000, true).unwrap();
        assert_eq!(plain.chunks, 2);
        assert_eq!(plain.strategy, SelectionStrategy::Exact);
        assert!(plain.polish.is_none());
        let report = polished.polish.as_ref().expect("batched runs are polished");
        assert_eq!(polished.strategy, SelectionStrategy::Exact);
        assert!(
            report.score_after <= report.score_before + 1e-12,
            "polish worsened the score: {} -> {}",
            report.score_before,
            report.score_after
        );
        assert!(polished.score <= plain.score + 1e-12);
        // Latin Hypercube: one design per bin per parameter, preserved
        // structurally by every swap
        for j in 0..15 {
            let mut bins: Vec<usize> = polished
                .new_designs
                .iter()
                .map(|d| (d.values[j] / 10.0 * 3.0) as usize)
                .collect();
            bins.sort_unstable();
            bins.dedup();
            assert_eq!(bins.len(), 3, "parameter {j}: designs share bins");
        }
    }

    #[test]
    fn next_iteration_detection() {
        assert_eq!(next_iteration(["0.1", "0.2", "0.30"]), 1);
        assert_eq!(next_iteration(["1.1", "0.2", "3.4"]), 4);
        assert_eq!(next_iteration(["d_1", "d_2"]), 1);
        assert_eq!(next_iteration([]), 1);
        // non-matching names are ignored even next to matching ones
        assert_eq!(next_iteration(["d_1", "2.2"]), 3);
        // malformed variants do not count
        assert_eq!(next_iteration(["a.b", "1.2.3", "1.x", ".5"]), 1);
    }

    #[test]
    fn augmentation_reports_the_selection_strategy() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
            Parameter::uniform("y", 0.0, 10.0)
                .unwrap()
                .with_scope(3.0, 7.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = (0..8)
            .map(|i| Design {
                name: format!("0.{}", i + 1),
                values: vec![2.2 + i as f64 * 0.5, 3.1 + (i % 5) as f64 * 0.8],
            })
            .collect();
        // few assignments on a small pool: exact enumeration runs
        let a = augment(&params, &designs, 2, 7, 1_000_000, false).unwrap();
        assert_eq!(a.strategy, SelectionStrategy::Exact);
        // cap 0 forces greedy
        let b = augment(&params, &designs, 2, 7, 0, false).unwrap();
        assert_eq!(b.strategy, SelectionStrategy::Greedy);
    }

    #[test]
    fn augment_single_uniform_parameter() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
        ];
        let designs = vec![
            Design {
                name: "0.1".into(),
                values: vec![2.5],
            },
            Design {
                name: "0.2".into(),
                values: vec![4.0],
            },
            Design {
                name: "0.3".into(),
                values: vec![7.0],
            },
        ];
        let aug = augment(&params, &designs, 2, 42, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), 2);
        assert_eq!(aug.iteration, 1);
        // no pairs with a single parameter, so the score is 0
        assert_eq!(aug.score, 0.0);
        for design in &aug.new_designs {
            assert!((2.0..=8.0).contains(&design.values[0]));
        }
    }

    #[test]
    fn augment_is_deterministic_for_a_seed() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
            Parameter::gaussian("y", 0.0, 10.0, 5.0, 2.0)
                .unwrap()
                .with_scope(3.0, 7.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = (0..12)
            .map(|i| Design {
                name: format!("0.{}", i + 1),
                values: vec![2.2 + i as f64 * 0.5, 3.1 + (i % 5) as f64 * 0.8],
            })
            .collect();
        let a = augment(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        let b = augment(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        assert_eq!(a.new_designs, b.new_designs);
        assert_eq!(a.score, b.score);
    }

    #[test]
    fn augment_rejects_bad_input() {
        let params = vec![Parameter::uniform("x", 0.0, 1.0).unwrap()];
        assert!(augment(&params, &[], 0, 1, 100, false).is_err());
        assert!(augment(&[], &[], 1, 1, 100, false).is_err());
        let bad = Design {
            name: "d".into(),
            values: vec![0.5, 0.5],
        };
        assert!(augment(&params, &[bad], 1, 1, 100, false).is_err());
        // too few shared in-scope designs for a pair
        let designs = vec![
            Design {
                name: "a".into(),
                values: vec![0.5, 0.5],
            },
            Design {
                name: "b".into(),
                values: vec![0.6, 0.6],
            },
        ];
        let two = vec![
            Parameter::uniform("x", 0.0, 1.0).unwrap(),
            Parameter::uniform("y", 0.0, 1.0).unwrap(),
        ];
        assert!(augment(&two, &designs, 1, 1, 100, false).is_err());
    }

    #[test]
    fn explore_covers_the_whole_surface_not_the_scope() {
        // gaussian parameter scoped to [4, 6], all designs clustered in
        // the middle of the scope
        let params = vec![
            Parameter::gaussian("x", 0.0, 10.0, 5.0, 2.0)
                .unwrap()
                .with_scope(4.0, 6.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = [5.0, 5.1, 5.2]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x],
            })
            .collect();

        let aug = explore(&params, &designs, 3, 42, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), 3);
        // 6 equal-width bins over the full [0, 10]; the three designs share
        // one bin, so the first three empty bins are 0, 1, 2 (single
        // parameter: no pairs, enumeration ties resolve lexicographically)
        for design in &aug.new_designs {
            assert!(
                (0.0..10.0).contains(&design.values[0]),
                "{} outside the definition range",
                design.values[0]
            );
            assert!(
                design.values[0] < 5.0,
                "{} inside the dense bin of the existing designs",
                design.values[0]
            );
        }
        // the new values lie outside the scope — that is the point of
        // explore mode
        assert!(
            aug.new_designs
                .iter()
                .any(|d| !(4.0..=6.0).contains(&d.values[0])),
            "explore produced only in-scope values"
        );
    }

    #[test]
    fn explore_pools_all_designs_of_the_definition_range() {
        // designs at 0.5 lie outside the declared scope [2, 8] but inside
        // the definition range; explore mode must count them
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
        ];
        let values = [
            0.5, 0.5, 2.0, 2.55, 3.1, 3.65, 4.2, 4.75, 5.3, 5.85, 6.4, 6.95,
        ];
        let designs: Vec<Design> = values
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x],
            })
            .collect();

        let aug = explore(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        // all 12 designs are in scope for the full-surface parameter:
        // 12 + 3 = 15 bins of width 10/15; the new values avoid the
        // occupied bins and stay inside the full range
        for design in &aug.new_designs {
            let x = design.values[0];
            assert!((0.0..=10.0).contains(&x));
            let bin = (x / 10.0 * 15.0).floor() as usize;
            let occupied = values
                .iter()
                .map(|&v| (v / 10.0 * 15.0).floor() as usize)
                .collect::<std::collections::BTreeSet<_>>();
            assert!(
                !occupied.contains(&bin),
                "value {x} landed in an occupied full-range bin"
            );
        }
        // names and iteration follow the same convention as augment
        assert_eq!(aug.iteration, 1);
        assert_eq!(aug.new_designs[0].name, "1.1");
    }

    #[test]
    fn explore_on_an_empty_pool_creates_all_new_designs() {
        // a fresh pool straight from `sampling init`: two example
        // parameters, no designs yet — the first `--explore` run must work
        let params = vec![
            Parameter::uniform("param_uniform", 0.0, 10.0).unwrap(),
            Parameter::gaussian("param_gaussian", 0.0, 10.0, 5.0, 2.0).unwrap(),
        ];
        let aug = explore(&params, &[], 10, 42, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), 10);
        assert_eq!(aug.iteration, 1);
        for (i, design) in aug.new_designs.iter().enumerate() {
            assert_eq!(design.name, format!("1.{}", i + 1));
            assert!((0.0..=10.0).contains(&design.values[0]));
            assert!((0.0..=10.0).contains(&design.values[1]));
        }
    }

    #[test]
    fn explore_with_many_designs_on_a_fresh_pool_works() {
        // regression for `sampling plhs --explore 1000` on a fresh pool:
        // the assignment count saturated u128 and the non-saturating
        // product panicked. Beyond the panic, the greedy fallback scored
        // every leaf in O(n); the incremental scorer keeps it O(1).
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0).unwrap(),
            Parameter::uniform("y", 0.0, 10.0).unwrap(),
        ];
        let m = 200;
        let aug = explore(&params, &[], m, 42, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), m);
        assert_eq!(aug.iteration, 1);
        assert!(aug.score.is_finite() && aug.score >= 0.0);
        for (i, design) in aug.new_designs.iter().enumerate() {
            assert_eq!(design.name, format!("1.{}", i + 1));
            assert!((0.0..=10.0).contains(&design.values[0]));
            assert!((0.0..=10.0).contains(&design.values[1]));
        }
        // Latin Hypercube: each new design occupies its own bin in every
        // parameter (m equal-width bins, empty pool)
        for j in 0..2 {
            let mut bins: Vec<usize> = aug
                .new_designs
                .iter()
                .map(|d| (d.values[j] / 10.0 * m as f64) as usize)
                .collect();
            bins.sort_unstable();
            bins.dedup();
            assert_eq!(bins.len(), m, "parameter {j}: designs share bins");
        }
    }

    #[test]
    fn augment_on_an_empty_pool_creates_all_new_designs() {
        // same code path via the scoped refinement: an empty pool has no
        // pair series yet, the new designs alone carry the Latin Hypercube
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0).unwrap(),
            Parameter::uniform("y", 0.0, 10.0).unwrap(),
        ];
        let aug = augment(&params, &[], 5, 42, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), 5);
        assert_eq!(aug.iteration, 1);
    }

    #[test]
    fn explore_on_a_fresh_pool_with_50_parameters_works() {
        // regression for issue #22: `sampling plhs --explore 6` on a fresh
        // 50-parameter pool used to fail on the greedy cross-product guard
        // (6^50 combinations). Batching degenerates to one design per
        // chunk — the standard-LHS loop — and the run succeeds.
        let params: Vec<Parameter> = (0..50)
            .map(|j| Parameter::uniform(format!("p{j}"), 0.0, 10.0).unwrap())
            .collect();
        let aug = explore(&params, &[], 6, 1, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), 6);
        assert_eq!(aug.iteration, 1);
        // 2^50 already exceeds the greedy budget: one design per chunk
        assert_eq!(aug.chunks, 6, "50 parameters must batch to m=1 chunks");
        // a chunk of one design on a fresh pool has a single assignment:
        // every chunk runs exact
        assert_eq!(aug.strategy, SelectionStrategy::Exact);
        assert!(aug.score.is_finite() && aug.score >= 0.0);
        for (i, design) in aug.new_designs.iter().enumerate() {
            assert_eq!(design.name, format!("1.{}", i + 1));
            assert_eq!(design.values.len(), 50);
            for value in &design.values {
                assert!((0.0..=10.0).contains(value));
            }
        }
        // Latin Hypercube: 6 equal-width bins per parameter, one design
        // per bin
        for j in 0..50 {
            let mut bins: Vec<usize> = aug
                .new_designs
                .iter()
                .map(|d| (d.values[j] / 10.0 * 6.0) as usize)
                .collect();
            bins.sort_unstable();
            bins.dedup();
            assert_eq!(bins.len(), 6, "parameter {j}: designs share bins");
        }
    }

    #[test]
    fn augment_with_50_parameters_and_a_scope_batches_too() {
        // the scoped refinement path takes the same batching; the values
        // stay inside the scope
        let params: Vec<Parameter> = (0..50)
            .map(|j| {
                Parameter::uniform(format!("p{j}"), 0.0, 10.0)
                    .unwrap()
                    .with_scope(2.0, 8.0)
                    .unwrap()
            })
            .collect();
        let aug = augment(&params, &[], 4, 1, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs.len(), 4);
        assert_eq!(aug.chunks, 4);
        for design in &aug.new_designs {
            assert_eq!(design.values.len(), 50);
            for value in &design.values {
                assert!(
                    (2.0..=8.0).contains(value),
                    "value {value} outside the scope"
                );
            }
        }
    }

    #[test]
    fn augmentation_that_fits_reports_a_single_chunk() {
        // small runs must keep the historical single-shot behaviour
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
            Parameter::gaussian("y", 0.0, 10.0, 5.0, 2.0)
                .unwrap()
                .with_scope(3.0, 7.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = (0..12)
            .map(|i| Design {
                name: format!("0.{}", i + 1),
                values: vec![2.2 + i as f64 * 0.5, 3.1 + (i % 5) as f64 * 0.8],
            })
            .collect();
        let aug = augment(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        assert_eq!(aug.chunks, 1);
        // deterministic as before
        let again = augment(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        assert_eq!(aug.new_designs, again.new_designs);
        assert_eq!(aug.score, again.score);
    }

    #[test]
    fn explore_is_deterministic_for_a_seed() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
            Parameter::gaussian("y", 0.0, 10.0, 5.0, 2.0)
                .unwrap()
                .with_scope(3.0, 7.0)
                .unwrap(),
        ];
        let designs: Vec<Design> = (0..12)
            .map(|i| Design {
                name: format!("0.{}", i + 1),
                values: vec![0.5 + i as f64 * 0.7, (i % 10) as f64 * 0.9 + 0.5],
            })
            .collect();
        let a = explore(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        let b = explore(&params, &designs, 3, 7, 1_000_000, false).unwrap();
        assert_eq!(a.new_designs, b.new_designs);
        assert_eq!(a.score, b.score);
    }
}
