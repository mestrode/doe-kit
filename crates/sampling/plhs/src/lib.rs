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
//!    greedy sequential selection beyond it.
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
//! let aug = augment(&params, &designs, 2, 42, 1_000_000)?;
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
//! let aug = explore(&params, &designs, 2, 42, 1_000_000)?;
//! // the new designs cover the full definition range [0.0, 10.0]
//! for design in &aug.new_designs {
//!     assert!((0.0..=10.0).contains(&design.values[0]));
//! }
//! # Ok::<(), plhs::Error>(())
//! ```

pub mod binning;
pub mod scoring;
pub mod selection;

pub use doe_types::error;
pub use doe_types::scaling;
pub use doe_types::{Design, Distribution, Error, Parameter, Result};

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use binning::{bin_count, candidate_t, empty_bins, occupied_bins};
use scoring::PairSeries;
use selection::{BinCandidates, select};

/// The result of [`augment`].
#[derive(Debug, Clone)]
pub struct Augmentation {
    /// The `m` new designs, in design order `1..=m`.
    pub new_designs: Vec<Design>,
    /// Score of the chosen assignment: sum of squared pair correlations.
    pub score: f64,
    /// Iteration number detected from the existing design names.
    pub iteration: u64,
}

/// Samples `m` new designs for the given parameters, extending the Latin
/// Hypercube inside each parameter's scope.
///
/// `cap` is the maximum number of assignments enumerated exactly before
/// falling back to greedy sequential selection.
///
/// # Errors
///
/// Returns an [`Error`] for any validation problem: duplicate parameters,
/// invalid scope entries, designs missing parameter values, parameter pairs
/// with fewer than three shared in-scope designs, and violated invariants.
pub fn augment(
    params: &[Parameter],
    designs: &[Design],
    m: usize,
    seed: u64,
    cap: u64,
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

    // Bins, empty bins and candidate values.
    let mut seed_bytes = [0u8; 32];
    seed_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let mut rng = ChaCha8Rng::from_seed(seed_bytes);
    let mut candidates: Vec<BinCandidates> = Vec::with_capacity(p);
    for (j, param) in params.iter().enumerate() {
        let bins = bin_count(in_scope[j].len(), m);
        let occupied = occupied_bins(&t_values[j], bins);
        let empty = empty_bins(bins, &occupied);
        if empty.len() < m {
            return Err(Error::Invariant(format!(
                "parameter '{}': {} empty bin(s) for {m} new design(s); \
                 the pigeonhole invariant (>= m empty bins) is violated",
                param.name,
                empty.len()
            )));
        }
        let mut t = Vec::with_capacity(empty.len());
        let mut values = Vec::with_capacity(empty.len());
        for &b in &empty {
            let ct = candidate_t(&mut rng, b, bins);
            t.push(ct);
            values.push(param.inverse_transform(ct));
        }
        candidates.push(BinCandidates {
            bins: empty,
            t,
            values,
        });
    }

    // Pair series for scoring: per pair, the designs in scope for both
    // parameters (transformed values).
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
            if xs.len() < 3 {
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

    // Selection.
    let selection = select(m, &pairs, &candidates, cap)?;

    // New designs and names.
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
            let values: Vec<f64> = (0..p)
                .map(|j| {
                    let pick = selection.picks[j][i];
                    candidates[j].values[pick]
                })
                .collect();
            Design {
                name: format!("{iteration}.{}", i + 1),
                values,
            }
        })
        .collect();

    Ok(Augmentation {
        new_designs,
        score: selection.score,
        iteration,
    })
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
/// `cap` and `seed` have the same meaning as in [`augment`].
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
) -> Result<Augmentation> {
    let full: Vec<Parameter> = params.iter().map(Parameter::full_surface).collect();
    augment(&full, designs, m, seed, cap)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let aug = augment(&params, &designs, 2, 42, 1_000_000).unwrap();
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
        let a = augment(&params, &designs, 3, 7, 1_000_000).unwrap();
        let b = augment(&params, &designs, 3, 7, 1_000_000).unwrap();
        assert_eq!(a.new_designs, b.new_designs);
        assert_eq!(a.score, b.score);
    }

    #[test]
    fn augment_rejects_bad_input() {
        let params = vec![Parameter::uniform("x", 0.0, 1.0).unwrap()];
        assert!(augment(&params, &[], 0, 1, 100).is_err());
        assert!(augment(&[], &[], 1, 1, 100).is_err());
        let bad = Design {
            name: "d".into(),
            values: vec![0.5, 0.5],
        };
        assert!(augment(&params, &[bad], 1, 1, 100).is_err());
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
        assert!(augment(&two, &designs, 1, 1, 100).is_err());
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

        let aug = explore(&params, &designs, 3, 42, 1_000_000).unwrap();
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

        let aug = explore(&params, &designs, 3, 7, 1_000_000).unwrap();
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
        let a = explore(&params, &designs, 3, 7, 1_000_000).unwrap();
        let b = explore(&params, &designs, 3, 7, 1_000_000).unwrap();
        assert_eq!(a.new_designs, b.new_designs);
        assert_eq!(a.score, b.score);
    }
}
