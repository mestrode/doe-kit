//! Removing individual designs from the pool (`sampling remove`).
//!
//! Two removal modes share this module:
//!
//! - named mode ([`remove_designs`]): the explicitly named designs go,
//!   all-or-nothing; a design with a results entry additionally needs
//!   `force` and then takes its entry with it.
//! - min-dist mode ([`remove_min_dist`]): redundant close-neighbor
//!   designs go. The threshold is the given `n` or — when
//!   `--min-dist` was given without a value — 10% of the pool's mean
//!   nearest-neighbor distance, frozen once at the start (removals
//!   raise the mean nearest-neighbor distance, a live threshold
//!   would feed back into ever more removals). Distances are
//!   Euclidean in the unit cube, every parameter linearly scaled to
//!   `[0, 1]` of its definition range — the normalization of the
//!   stats metrics. One removal per iteration, always from the
//!   globally closest actionable pair, so each decision sees the new
//!   situation; ties order by the lexicographic pair names. The victim
//!   of a pair mixing an evaluated and an unevaluated design is always
//!   the unevaluated one; between same-status designs the one with
//!   the smaller nearest-other-design distance is the more redundant
//!   and goes, ties by the lexicographically first name. Evaluated
//!   designs are protected unless `force` lifts the protection — then
//!   they can also be victims and their results entries go with them.

use std::collections::BTreeSet;

use serde_json::Value;

use doe_types::error::{Error, Result};
use doe_types::{Design, Parameter};

use crate::format::parse_results;

/// 10% of the pool's mean nearest-neighbor distance: the default
/// neighbor threshold of `--min-dist` (matches the pool judgment rule
/// that the minimum pairwise distance should be roughly 10% of the
/// mean nearest-neighbor distance).
const DEFAULT_THRESHOLD_FRACTION: f64 = 0.1;

fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

/// Outcome of a removal ([`remove_designs`] / [`remove_min_dist`]): the
/// design and results counts before and after, plus the effective
/// neighbor threshold (min-dist mode only; `None` in named mode).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoveReport {
    /// Designs in the pool before the removal.
    pub designs_before: usize,
    /// Designs in the pool after the removal.
    pub designs_after: usize,
    /// Results entries before the removal.
    pub results_before: usize,
    /// Results entries after the removal (drops when removed designs
    /// carried entries — only possible with `force`).
    pub results_after: usize,
    /// The effective absolute distance threshold of a min-dist run
    /// (the given `n`, or 10% of the pool's mean nearest-neighbor
    /// distance when `--min-dist` was given without a value). `None`
    /// in named mode and when the pool has fewer than two designs and
    /// no explicit threshold was given.
    pub threshold: Option<f64>,
}

/// Design names of the pool, in pool order. The caller (the bin)
/// validates the pool structure before; here a non-string design
/// name is still a validation error, nothing is removed silently.
fn design_names(root: &Value) -> Result<Vec<String>> {
    match root.get("designs") {
        Some(Value::Array(entries)) => entries
            .iter()
            .map(|d| {
                d.get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("'design.name' must be a string"))
            })
            .collect(),
        Some(_) => Err(invalid("'designs' must be an array")),
        None => Err(invalid("'root' is missing the field 'designs'")),
    }
}

/// Drops every design whose name is in `names` together with the
/// results entries of those designs (an orphaned entry would
/// invalidate the pool: `stats` validates result names against
/// designs). Entries naming other designs — including entries that
/// already matched no design — pass through untouched, as do all
/// other fields and the key order.
fn drop_named(root: &mut Value, names: &BTreeSet<&str>) {
    let root_obj = match root.as_object_mut() {
        Some(obj) => obj,
        None => return,
    };
    if let Some(designs) = root_obj.get_mut("designs").and_then(Value::as_array_mut) {
        designs.retain(|d| {
            d.get("name")
                .and_then(Value::as_str)
                .is_none_or(|n| !names.contains(n))
        });
    }
    if let Some(results) = root_obj.get_mut("results").and_then(Value::as_array_mut) {
        results.retain(|r| {
            r.get("name")
                .and_then(Value::as_str)
                .is_none_or(|n| !names.contains(n))
        });
    }
}

/// Counts the designs and results entries left in the pool.
fn count_after(root: &Value) -> (usize, usize) {
    let designs = root
        .get("designs")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let results = root
        .get("results")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    (designs, results)
}

/// Removes the named designs from the pool (named mode).
///
/// All-or-nothing: every name must match a design — otherwise a
/// validation error lists the unknown names and nothing is removed.
/// A named design that carries a results entry additionally requires
/// `force`; without it the error lists every offending name and
/// nothing is removed. With `force` the design is removed together
/// with its results entry. Everything else — parameters, other
/// designs, other results entries, unknown fields, key order — is
/// untouched.
pub fn remove_designs(root: &mut Value, names: &[String], force: bool) -> Result<RemoveReport> {
    let results = parse_results(root)?;
    let solved: BTreeSet<&str> = results.iter().map(|r| r.name.as_str()).collect();
    let results_before = results.len();
    let existing = design_names(root)?;
    let designs_before = existing.len();

    // deduplicated, order preserved
    let requested: Vec<&str> = {
        let mut seen = BTreeSet::new();
        names
            .iter()
            .filter(|n| seen.insert(n.as_str()))
            .map(|n| n.as_str())
            .collect()
    };

    // all-or-nothing: every name must match a design
    let unknown: Vec<&str> = requested
        .iter()
        .filter(|n| !existing.iter().any(|e| e == *n))
        .copied()
        .collect();
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "unknown design(s): {}",
            unknown.join(", ")
        )));
    }

    // designs with results entries need --force, all offenders listed
    if !force {
        let offenders: Vec<&str> = requested
            .iter()
            .filter(|n| solved.contains(*n))
            .copied()
            .collect();
        if !offenders.is_empty() {
            return Err(invalid(format!(
                "design(s) {} have results entries; use --force to remove",
                offenders.join(", ")
            )));
        }
    }

    let removed: BTreeSet<&str> = requested.into_iter().collect();
    drop_named(root, &removed);
    let (designs_after, results_after) = count_after(root);
    Ok(RemoveReport {
        designs_before,
        designs_after,
        results_before,
        results_after,
        threshold: None,
    })
}

/// Euclidean distance between two points of the unit cube (the
/// normalization of the stats metrics).
fn distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// Mean over all designs of the nearest-neighbor distance (the
/// `stats` mean nearest neighbor, unit cube).
fn mean_nearest_neighbor(unit: &[Vec<f64>]) -> f64 {
    let n = unit.len();
    let sum: f64 = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| j != i)
                .map(|j| distance(&unit[i], &unit[j]))
                .fold(f64::INFINITY, f64::min)
        })
        .sum();
    sum / n as f64
}

/// The state of one min-dist iteration: which designs are still in the
/// pool, whether they carry results, and their unit-cube points.
struct Neighbors<'a> {
    names: &'a [String],
    evaluated: &'a [bool],
    unit: &'a [Vec<f64>],
    removed: Vec<bool>,
}

impl Neighbors<'_> {
    /// Nearest distance from `x` to any design other than `partner`
    /// (and other than the already removed designs); infinity when no
    /// such design is left.
    fn nearest_other(&self, x: usize, partner: usize) -> f64 {
        (0..self.names.len())
            .filter(|&k| k != x && k != partner && !self.removed[k])
            .map(|k| distance(&self.unit[x], &self.unit[k]))
            .fold(f64::INFINITY, f64::min)
    }

    /// The victim of the pair: the unevaluated member of a
    /// mixed-status pair, otherwise the design more redundant with
    /// the rest of the pool (smaller nearest-other distance), ties by
    /// the lexicographically first name.
    fn victim(&self, i: usize, j: usize) -> usize {
        if self.evaluated[i] != self.evaluated[j] {
            return if self.evaluated[i] { j } else { i };
        }
        let (di, dj) = (self.nearest_other(i, j), self.nearest_other(j, i));
        if di != dj {
            return if di < dj { i } else { j };
        }
        if self.names[i] <= self.names[j] { i } else { j }
    }
}

/// Removes redundant close-neighbor designs from the pool (min-dist
/// mode). See the module documentation for the algorithm; the report
/// carries the frozen effective threshold.
pub fn remove_min_dist(
    root: &mut Value,
    params: &[Parameter],
    threshold: Option<f64>,
    force: bool,
) -> Result<RemoveReport> {
    if let Some(n) = threshold {
        if n <= 0.0 {
            return Err(invalid("the distance threshold must be greater than zero"));
        }
    }
    let results = parse_results(root)?;
    let solved: BTreeSet<&str> = results.iter().map(|r| r.name.as_str()).collect();
    let results_before = results.len();
    let designs: Vec<Design> = crate::format::parse_designs(root, params)?;
    let designs_before = designs.len();
    let evaluated: Vec<bool> = designs
        .iter()
        .map(|d| solved.contains(d.name.as_str()))
        .collect();
    // unit cube: every parameter linearly scaled to [0, 1] of its
    // definition range (the definition range, not the scope)
    let unit: Vec<Vec<f64>> = designs
        .iter()
        .map(|d| {
            d.values
                .iter()
                .zip(params)
                .map(|(x, p)| (x - p.min) / (p.max - p.min))
                .collect()
        })
        .collect();
    let names: Vec<String> = designs.iter().map(|d| d.name.clone()).collect();

    // the threshold is frozen once: removals raise the mean
    // nearest-neighbor distance, recomputing it would feed back
    let effective = match threshold {
        Some(n) => Some(n),
        None if designs.len() >= 2 => {
            Some(DEFAULT_THRESHOLD_FRACTION * mean_nearest_neighbor(&unit))
        }
        None => None,
    };

    let mut state = Neighbors {
        names: &names,
        evaluated: &evaluated,
        unit: &unit,
        removed: vec![false; designs.len()],
    };

    if let Some(thr) = effective {
        loop {
            // the globally closest actionable pair: both remaining,
            // distance strictly below the threshold, and (without
            // force) at least one unevaluated member
            let mut best: Option<(f64, usize, usize)> = None;
            for i in 0..designs.len() {
                if state.removed[i] {
                    continue;
                }
                for j in (i + 1)..designs.len() {
                    if state.removed[j] {
                        continue;
                    }
                    if !force && evaluated[i] && evaluated[j] {
                        continue;
                    }
                    let d = distance(&unit[i], &unit[j]);
                    if d >= thr {
                        continue;
                    }
                    match best {
                        None => best = Some((d, i, j)),
                        Some((bd, bi, bj)) => {
                            let closer = d < bd || {
                                // tie: the lexicographically smaller
                                // (sorted) name pair wins
                                d == bd && {
                                    let (a, b) = (&names[i], &names[j]);
                                    let (ba, bb) = (&names[bi], &names[bj]);
                                    (a.min(b), a.max(b)) < (ba.min(bb), ba.max(bb))
                                }
                            };
                            if closer {
                                best = Some((d, i, j));
                            }
                        }
                    }
                }
            }
            let Some((_, i, j)) = best else {
                break;
            };
            let victim = state.victim(i, j);
            state.removed[victim] = true;
        }
    }

    let removed: BTreeSet<&str> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| state.removed[*i])
        .map(|(_, name)| name.as_str())
        .collect();
    drop_named(root, &removed);
    let (designs_after, results_after) = count_after(root);
    Ok(RemoveReport {
        designs_before,
        designs_after,
        results_before,
        results_after,
        threshold: effective,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A pool with two uniform parameters `x`, `y` over `[0, 10]` and the
    /// given designs (name, x, y); `solved` lists the designs with a
    /// results entry.
    fn pool(designs: &[(&str, f64, f64)], solved: &[&str]) -> Value {
        json!({
            "params": [
                { "name": "x", "min": 0, "max": 10, "distribution": "uniform" },
                { "name": "y", "min": 0, "max": 10, "distribution": "uniform" }
            ],
            "designs": designs
                .iter()
                .map(|(name, x, y)| json!({ "name": name, "x": x, "y": y }))
                .collect::<Vec<_>>(),
            "results": solved
                .iter()
                .map(|name| json!({ "name": name, "linear": 1.0 }))
                .collect::<Vec<_>>(),
            "note": "unknown fields must survive"
        })
    }

    fn params_of(root: &Value) -> Vec<Parameter> {
        crate::format::parse_params(root).unwrap()
    }

    fn remaining_names(root: &Value) -> Vec<String> {
        root["designs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap().to_owned())
            .collect()
    }

    fn solved_names(root: &Value) -> Vec<String> {
        match root.get("results") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|r| r["name"].as_str().unwrap().to_owned())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    // -- named mode -------------------------------------------------------

    #[test]
    fn named_removes_the_designs_and_keeps_everything_else() {
        let mut root = pool(
            &[("0.1", 1.0, 1.0), ("0.2", 2.0, 2.0), ("0.3", 3.0, 3.0)],
            &["0.2"],
        );
        let report = remove_designs(&mut root, &names(&["0.1", "0.3"]), false).unwrap();
        assert_eq!(
            report,
            RemoveReport {
                designs_before: 3,
                designs_after: 1,
                results_before: 1,
                results_after: 1,
                threshold: None,
            }
        );
        assert_eq!(remaining_names(&root), ["0.2"]);
        assert_eq!(solved_names(&root), ["0.2"], "the untouched entry stays");
        assert_eq!(root["note"], "unknown fields must survive");
    }

    #[test]
    fn named_refuses_unknown_names_listing_all_of_them() {
        let mut root = pool(&[("0.1", 1.0, 1.0), ("0.2", 2.0, 2.0)], &[]);
        let err = remove_designs(&mut root, &names(&["9.9", "8.8", "0.1"]), false).unwrap_err();
        assert!(err.to_string().contains("9.9"), "{err}");
        assert!(err.to_string().contains("8.8"), "{err}");
        assert!(err.to_string().contains("unknown"), "{err}");
        // all-or-nothing: the valid name was not removed either
        assert_eq!(remaining_names(&root), ["0.1", "0.2"]);
    }

    #[test]
    fn named_refuses_results_entries_without_force_listing_all_offenders() {
        let mut root = pool(
            &[("0.1", 1.0, 1.0), ("0.2", 2.0, 2.0), ("0.3", 3.0, 3.0)],
            &["0.1", "0.2"],
        );
        let err = remove_designs(&mut root, &names(&["0.1", "0.2", "0.3"]), false).unwrap_err();
        assert!(err.to_string().contains("0.1"), "{err}");
        assert!(err.to_string().contains("0.2"), "{err}");
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(remaining_names(&root), ["0.1", "0.2", "0.3"]);
        assert_eq!(solved_names(&root).len(), 2);
    }

    #[test]
    fn named_with_force_removes_the_designs_and_their_results_entries() {
        let mut root = pool(
            &[("0.1", 1.0, 1.0), ("0.2", 2.0, 2.0), ("0.3", 3.0, 3.0)],
            &["0.1", "0.2"],
        );
        let report = remove_designs(&mut root, &names(&["0.1", "0.2"]), true).unwrap();
        assert_eq!(
            report,
            RemoveReport {
                designs_before: 3,
                designs_after: 1,
                results_before: 2,
                results_after: 0,
                threshold: None,
            }
        );
        assert_eq!(remaining_names(&root), ["0.3"]);
        assert!(solved_names(&root).is_empty(), "the entries went with them");
    }

    #[test]
    fn named_deduplicates_repeated_names() {
        let mut root = pool(&[("0.1", 1.0, 1.0), ("0.2", 2.0, 2.0)], &[]);
        let report = remove_designs(&mut root, &names(&["0.1", "0.1"]), false)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(report.designs_after, 1);
        assert_eq!(remaining_names(&root), ["0.2"]);
    }

    #[test]
    fn named_on_an_empty_name_list_is_a_no_op() {
        let mut root = pool(&[("0.1", 1.0, 1.0)], &[]);
        let report = remove_designs(&mut root, &[], false).unwrap();
        assert_eq!(report.designs_before, 1);
        assert_eq!(report.designs_after, 1);
        assert_eq!(remaining_names(&root), ["0.1"]);
    }

    // -- min-dist mode ----------------------------------------------------

    /// The golden geometry: an evaluated design with an unevaluated
    /// near-duplicate, an unevaluated pair of close neighbors, and an
    /// isolated design.
    fn wide_pool() -> Value {
        pool(
            &[
                ("0.1", 1.0, 1.0),
                ("0.2", 1.05, 1.0),
                ("0.3", 5.0, 5.0),
                ("0.4", 5.06, 5.0),
                ("0.5", 9.0, 9.0),
            ],
            &["0.1"],
        )
    }

    #[test]
    fn min_dist_removes_duplicates_and_the_redundant_partner() {
        let mut root = wide_pool();
        let params = params_of(&root);
        let report = remove_min_dist(&mut root, &params, Some(0.01), false).unwrap();
        assert_eq!(
            report,
            RemoveReport {
                designs_before: 5,
                designs_after: 3,
                results_before: 1,
                results_after: 1,
                threshold: Some(0.01),
            }
        );
        // 0.2 is the unevaluated partner of the evaluated 0.1; 0.4 is
        // the design more redundant with the rest of the pool
        assert_eq!(remaining_names(&root), ["0.1", "0.3", "0.5"]);
        assert_eq!(solved_names(&root), ["0.1"]);
        assert_eq!(root["note"], "unknown fields must survive");
    }

    #[test]
    fn min_dist_thins_a_clump_to_a_single_design() {
        let mut root = pool(
            &[("1.1", 1.0, 1.0), ("1.2", 1.1, 1.0), ("1.3", 1.2, 1.0)],
            &[],
        );
        let params = params_of(&root);
        let report = remove_min_dist(&mut root, &params, Some(0.05), false).unwrap();
        assert_eq!(report.designs_after, 1, "one design survives the clump");
        assert_eq!(remaining_names(&root), ["1.3"]);
    }

    #[test]
    fn min_dist_default_threshold_is_ten_percent_of_the_mean_nn() {
        let mut root = pool(
            &[("1.1", 0.0, 0.0), ("1.2", 0.01, 0.0), ("1.3", 0.5, 0.5)],
            &[],
        );
        let params = params_of(&root);
        let report = remove_min_dist(&mut root, &params, None, false).unwrap();
        // mean NN = (0.001 + 0.001 + d(1.3, nearest)) / 3 over the
        // unit cube; 10% of that is still above the 0.001 duplicate
        let d_13 = (0.05f64 * 0.05 + 0.049 * 0.049).sqrt();
        let expected = 0.1 * (0.001 + 0.001 + d_13) / 3.0;
        assert!(
            (report.threshold.unwrap() - expected).abs() < 1e-12,
            "threshold {} vs expected {expected}",
            report.threshold.unwrap()
        );
        assert_eq!(report.designs_after, 2, "the duplicate is removed");
        assert_eq!(remaining_names(&root), ["1.1", "1.3"]);
    }

    #[test]
    fn min_dist_without_force_never_touches_evaluated_designs() {
        let mut root = pool(
            &[("0.1", 1.0, 1.0), ("0.2", 1.02, 1.0), ("0.3", 9.0, 9.0)],
            &["0.1", "0.2"],
        );
        let params = params_of(&root);
        let report = remove_min_dist(&mut root, &params, Some(0.01), false).unwrap();
        assert_eq!(report.designs_after, 3, "the close pair is protected");
        assert_eq!(report.results_after, 2);
    }

    #[test]
    fn min_dist_with_force_removes_the_more_redundant_evaluated_design() {
        let mut root = pool(
            &[("0.1", 1.0, 1.0), ("0.2", 1.02, 1.0), ("0.3", 9.0, 9.0)],
            &["0.1", "0.2"],
        );
        let params = params_of(&root);
        let report = remove_min_dist(&mut root, &params, Some(0.01), true).unwrap();
        assert_eq!(
            report,
            RemoveReport {
                designs_before: 3,
                designs_after: 2,
                results_before: 2,
                results_after: 1,
                threshold: Some(0.01),
            }
        );
        // 0.2 has the smaller nearest-other distance (closer to 0.3)
        assert_eq!(remaining_names(&root), ["0.1", "0.3"]);
        assert_eq!(solved_names(&root), ["0.1"], "the entry went with it");
    }

    #[test]
    fn min_dist_on_a_single_design_pool_is_a_no_op() {
        let mut root = pool(&[("1.1", 1.0, 1.0)], &[]);
        let params = params_of(&root);
        let report = remove_min_dist(&mut root, &params, None, false).unwrap();
        assert_eq!(report.designs_after, 1);
        assert_eq!(report.threshold, None, "the mean NN is undefined");
        let report = remove_min_dist(&mut root, &params, Some(0.01), false).unwrap();
        assert_eq!(report.threshold, Some(0.01));
    }

    #[test]
    fn min_dist_refuses_a_non_positive_threshold() {
        let mut root = wide_pool();
        let params = params_of(&root);
        for bad in [0.0, -0.5] {
            let err = remove_min_dist(&mut root, &params, Some(bad), false).unwrap_err();
            assert!(err.to_string().contains("greater than zero"), "{err}");
        }
        assert_eq!(
            remaining_names(&root).len(),
            5,
            "nothing was removed on the errors"
        );
    }
}
