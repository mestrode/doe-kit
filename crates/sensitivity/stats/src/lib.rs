//! Statistical analysis of evaluated Design of Experiments results.
//!
//! Judgment support for the sampling workflow: before expensive simulations
//! are run on new designs, [`analyze`] summarizes what the already evaluated
//! designs say about each result variable — how well the pool is covered,
//! how each parameter relates to the result, and where along each
//! parameter's definition range the results are good or bad.
//!
//! The analysis is statistics only; the judgment stays with the caller.
//!
//! Two analyses are provided:
//!
//! - [`analyze_pool`]: statistics of the sampling points themselves
//!   (the pool's designs) — per-parameter descriptive statistics, decile
//!   coverage and outer-decile counts of the definition range, space-
//!   filling metrics in the unit cube (min pairwise distance, mean
//!   nearest-neighbor distance, centered L2 discrepancy) and Pearson and
//!   Spearman correlation per parameter pair. It answers "how well are
//!   the generated sampling points distributed, and do they correlate?"
//!   before anything has been evaluated.
//! - Per result variable: coverage (evaluated vs total designs) and
//!   descriptive statistics (min, max, mean, sample std, median).
//! - Per parameter: Pearson and Spearman correlation with the result
//!   (Spearman catches monotonic-but-nonlinear trends), mean/std of the
//!   results inside the parameter's scope vs outside it, and the result
//!   mean per decile of the definition range.
//!
//! Correlations are reported as `None` when fewer than 3 evaluated designs
//! exist or either series has zero variance — the numbers would be noise.

use std::collections::{BTreeMap, BTreeSet};

use doe_types::error::{Error, Result};
use doe_types::{Design, Parameter};
use plhs::scoring::pearson;

pub use doe_types::Results;

/// Descriptive statistics of one result variable.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VariableStats {
    /// Variable name (the result field, e.g. `energy`).
    pub name: String,
    /// Evaluated designs that carry this variable.
    pub n: usize,
    /// Total designs in the pool.
    pub total: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    /// Sample standard deviation (n - 1; 0.0 for n < 2).
    pub std: f64,
    pub median: f64,
    /// Per-parameter statistics, aligned with the input parameter slice.
    pub parameters: Vec<ParameterStats>,
}

/// Per-parameter statistics for one result variable.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ParameterStats {
    /// Parameter name.
    pub name: String,
    /// Pearson correlation with the result; `None` when undefined
    /// (n < 3, zero variance in either series).
    pub pearson: Option<f64>,
    /// Spearman rank correlation; same conditions as [`Self::pearson`].
    pub spearman: Option<f64>,
    /// Mean/std of the result over evaluated designs inside the scope.
    pub in_scope: MeanStd,
    /// Mean/std over evaluated designs outside the scope; `None` when no
    /// evaluated design lies outside (nothing to compare against).
    pub out_of_scope: Option<MeanStd>,
    /// Result mean per decile (10 equal-width bins) of the definition
    /// range; empty deciles carry count 0. Always 10 entries.
    pub deciles: Vec<Decile>,
}

/// Count, mean and sample std of a result over a set of designs.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MeanStd {
    /// Number of designs in the set.
    pub count: usize,
    pub mean: f64,
    pub std: f64,
}

/// One decile bin of the definition range.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Decile {
    /// Evaluated designs in this decile.
    pub count: usize,
    /// Mean result over them; `None` for an empty decile.
    pub mean: Option<f64>,
}

/// Number of deciles per parameter definition range.
pub const DECILES: usize = 10;

/// Sampling-point statistics of the pool itself, independent of any
/// evaluated results.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PoolStats {
    /// Total designs in the pool.
    pub total: usize,
    /// Per-parameter statistics, aligned with the input parameter slice.
    pub parameters: Vec<ParameterPoolStats>,
    /// Smallest pairwise Euclidean distance between two designs in the
    /// unit cube (every parameter scaled to `[0, 1]` of its definition
    /// range); `None` for fewer than 2 designs. Near zero means
    /// near-duplicate parameter sets.
    pub min_pairwise_distance: Option<f64>,
    /// Mean over all designs of the distance to the nearest other design,
    /// in the same normalized space; `None` for fewer than 2 designs.
    pub mean_nearest_neighbor: Option<f64>,
    /// Centered L2 discrepancy (Hickernell) of the sampling points in
    /// the same normalized space: a single uniformity number for the
    /// whole pool, comparable across seeds and pools. Lower is better.
    pub discrepancy_cl2: f64,
    /// Correlation between the sampling points of each parameter pair
    /// (`a` precedes `b` in the input parameter order).
    pub pairs: Vec<PairCorrelation>,
}

/// Descriptive statistics of one parameter's sampling points.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ParameterPoolStats {
    /// Parameter name.
    pub name: String,
    /// Designs in the pool (all of them carry this parameter).
    pub n: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    /// Sample standard deviation (n - 1; 0.0 for n < 2).
    pub std: f64,
    pub median: f64,
    /// Number of sampling points per decile (10 equal-width bins) of the
    /// definition range — how uniformly the range is covered. Empty
    /// deciles carry count 0. Always 10 entries.
    pub deciles: Vec<usize>,
    /// Sampling points in the outer deciles: the sum of the first and
    /// the last decile count — whether the range edges are populated.
    pub outer_deciles: usize,
}

/// Correlation between the sampling points of two parameters.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PairCorrelation {
    /// First parameter of the pair.
    pub a: String,
    /// Second parameter of the pair.
    pub b: String,
    /// Pearson correlation; `None` when undefined (n < 3, zero variance
    /// in either series).
    pub pearson: Option<f64>,
    /// Spearman rank correlation; same conditions as [`Self::pearson`].
    pub spearman: Option<f64>,
}

/// Analyzes the pool's sampling points (`designs`) themselves: how well
/// they cover each parameter's definition range and how the parameters
/// correlate with each other. Independent of any evaluated results —
/// this is what to look at right after `plhs` generated new designs.
///
/// # Errors
///
/// - `designs` is empty: there is nothing to analyze.
/// - A design lacks a value for a declared parameter.
pub fn analyze_pool(params: &[Parameter], designs: &[Design]) -> Result<PoolStats> {
    if designs.is_empty() {
        return Err(Error::Invalid("no designs to analyze".to_owned()));
    }
    let p = params.len();
    for design in designs {
        if design.values.len() != p {
            return Err(Error::Invalid(format!(
                "design '{}' has {} value(s), expected one per parameter ({p})",
                design.name,
                design.values.len()
            )));
        }
    }

    let correlation = |xs: &[f64], ys: &[f64], n: usize| -> Option<f64> {
        if n < 3 || variance_is_zero(xs) || variance_is_zero(ys) {
            return None;
        }
        Some(pearson(xs, ys))
    };

    let mut parameters = Vec::with_capacity(p);
    for (j, param) in params.iter().enumerate() {
        let xs: Vec<f64> = designs.iter().map(|d| d.values[j]).collect();
        let (min, max, mean, std, median) = descriptive(&xs);
        let width = (param.max - param.min) / DECILES as f64;
        let mut deciles = vec![0usize; DECILES];
        for d in designs {
            let x = d.values[j];
            let bin = if x >= param.max {
                DECILES - 1
            } else {
                ((x - param.min) / width) as usize
            };
            deciles[bin] += 1;
        }
        let outer_deciles = deciles[0] + deciles[DECILES - 1];
        parameters.push(ParameterPoolStats {
            name: param.name.clone(),
            n: xs.len(),
            min,
            max,
            mean,
            std,
            median,
            deciles,
            outer_deciles,
        });
    }

    // unit-cube coordinates: every parameter linearly scaled to [0, 1]
    // of its definition range (the definition range, not the scope), so
    // distances and discrepancy are comparable across parameters with
    // different units
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
    let (min_pairwise_distance, mean_nearest_neighbor) = if designs.len() < 2 {
        (None, None)
    } else {
        let mut min_pair = f64::INFINITY;
        let mut nn_sum = 0.0;
        for (i, u) in unit.iter().enumerate() {
            let mut nearest = f64::INFINITY;
            for (j, v) in unit.iter().enumerate() {
                if i == j {
                    continue;
                }
                let d = distance(u, v);
                min_pair = min_pair.min(d);
                nearest = nearest.min(d);
            }
            nn_sum += nearest;
        }
        (Some(min_pair), Some(nn_sum / designs.len() as f64))
    };
    let discrepancy_cl2 = centered_l2_discrepancy(&unit);

    let n = designs.len();
    let mut pairs = Vec::with_capacity(p * (p - 1) / 2);
    for a in 0..p {
        for b in (a + 1)..p {
            let xs: Vec<f64> = designs.iter().map(|d| d.values[a]).collect();
            let ys: Vec<f64> = designs.iter().map(|d| d.values[b]).collect();
            let pearson = correlation(&xs, &ys, n);
            let spearman = correlation(&ranks(&xs), &ranks(&ys), n);
            pairs.push(PairCorrelation {
                a: params[a].name.clone(),
                b: params[b].name.clone(),
                pearson,
                spearman,
            });
        }
    }

    Ok(PoolStats {
        total: designs.len(),
        parameters,
        min_pairwise_distance,
        mean_nearest_neighbor,
        discrepancy_cl2,
        pairs,
    })
}

/// Euclidean distance between two points of the unit cube.
fn distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// Centered L2 discrepancy (Hickernell) of points in the unit cube:
///
/// `CL2 = sqrt((13/12)^k - (2/n) * sum_i prod_k(1 + d_ik - d_ik^2)`
/// `      + (1/n^2) * sum_i sum_j prod_k(1 + d_ik + d_jk - g_ijk / 2))`
///
/// with `d = |u - 1/2|` and `g = |u_i - u_j|`. A single uniformity
/// number for the whole point set; lower is better, and it is directly
/// comparable between candidate pools (e.g. different seeds). The
/// argument must be non-empty.
fn centered_l2_discrepancy(unit: &[Vec<f64>]) -> f64 {
    let n = unit.len() as f64;
    let k = unit.first().map_or(0, |u| u.len());
    let base = (13.0_f64 / 12.0).powi(k as i32);
    let single = unit
        .iter()
        .map(|u| {
            u.iter()
                .map(|&x| {
                    let d = (x - 0.5).abs();
                    1.0 + d - d * d
                })
                .product::<f64>()
        })
        .sum::<f64>();
    let mut pair_sum = 0.0;
    for u in unit {
        for v in unit {
            pair_sum += u
                .iter()
                .zip(v)
                .map(|(&a, &b)| {
                    let da = (a - 0.5).abs();
                    let db = (b - 0.5).abs();
                    1.0 + da + db - (a - b).abs() / 2.0
                })
                .product::<f64>();
        }
    }
    let squared = base - 2.0 * single / n + pair_sum / (n * n);
    squared.max(0.0).sqrt()
}

/// (min, max, mean, sample std, median) of a finite series.
fn descriptive(xs: &[f64]) -> (f64, f64, f64, f64, f64) {
    let n = xs.len();
    let mut sorted = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("values are finite"));
    let mean = xs.iter().sum::<f64>() / n as f64;
    let std = mean_std(xs).std;
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    (sorted[0], sorted[n - 1], mean, std, median)
}

/// Analyzes the evaluated `results` against the pool (`params`, `designs`).
///
/// One [`VariableStats`] per numeric result field, in alphabetical order.
///
/// # Errors
///
/// - `results` is empty: there is nothing to analyze.
/// - A result name matches no design in the pool.
/// - A result entry carries no numeric value at all.
/// - A referenced design lacks a value for a declared parameter.
pub fn analyze(
    params: &[Parameter],
    designs: &[Design],
    results: &[Results],
) -> Result<Vec<VariableStats>> {
    if results.is_empty() {
        return Err(Error::Invalid("no results to analyze".to_owned()));
    }
    let index: BTreeMap<&str, usize> = designs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name.as_str(), i))
        .collect();
    for r in results {
        if !index.contains_key(r.name.as_str()) {
            return Err(Error::Invalid(format!(
                "result '{}' does not match any design",
                r.name
            )));
        }
    }
    let variables: Vec<&String> = results
        .iter()
        .flat_map(|r| r.values.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if variables.is_empty() {
        return Err(Error::Invalid(
            "result entries contain no numeric values to analyze".to_owned(),
        ));
    }

    let mut out = Vec::with_capacity(variables.len());
    for variable in variables {
        // evaluated designs carrying this variable, with (design, value)
        let mut pairs: Vec<(&Design, f64)> = Vec::new();
        for r in results {
            if let Some(&v) = r.values.get(variable) {
                let &i = index
                    .get(r.name.as_str())
                    .ok_or_else(|| Error::Invariant("result name lost".to_owned()))?;
                let design = &designs[i];
                if design.values.len() != params.len() {
                    return Err(Error::Invalid(format!(
                        "design '{}' has {} value(s), expected one per parameter ({})",
                        design.name,
                        design.values.len(),
                        params.len()
                    )));
                }
                pairs.push((design, v));
            }
        }
        let ys: Vec<f64> = pairs.iter().map(|&(_, v)| v).collect();

        // descriptive statistics
        let mut sorted = ys.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("results are finite"));
        let n = ys.len();
        let mean = ys.iter().sum::<f64>() / n as f64;
        let std = mean_std(&ys).std;
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        };

        // per-parameter statistics
        let mut parameter_stats = Vec::with_capacity(params.len());
        for (j, param) in params.iter().enumerate() {
            let xs: Vec<f64> = pairs.iter().map(|&(d, _)| d.values[j]).collect();

            let correlation = |xs: &[f64], ys: &[f64]| -> Option<f64> {
                if n < 3 || variance_is_zero(xs) || variance_is_zero(ys) {
                    return None;
                }
                Some(pearson(xs, ys))
            };
            let p = correlation(&xs, &ys);
            let s = correlation(&ranks(&xs), &ranks(&ys));

            let mut in_scope: Vec<f64> = Vec::new();
            let mut out: Vec<f64> = Vec::new();
            for &(design, value) in &pairs {
                if param.in_scope(design.values[j]) {
                    in_scope.push(value);
                } else {
                    out.push(value);
                }
            }
            let out_of_scope = if out.is_empty() {
                None
            } else {
                Some(mean_std(&out))
            };

            let width = (param.max - param.min) / DECILES as f64;
            let mut deciles: Vec<(usize, f64)> = vec![(0, 0.0); DECILES];
            for &(design, value) in &pairs {
                let x = design.values[j];
                let bin = if x >= param.max {
                    DECILES - 1
                } else {
                    ((x - param.min) / width) as usize
                };
                deciles[bin].0 += 1;
                deciles[bin].1 += value;
            }
            let deciles: Vec<Decile> = deciles
                .into_iter()
                .map(|(count, sum)| Decile {
                    count,
                    mean: (count > 0).then_some(sum / count as f64),
                })
                .collect();

            parameter_stats.push(ParameterStats {
                name: param.name.clone(),
                pearson: p,
                spearman: s,
                in_scope: mean_std(&in_scope),
                out_of_scope,
                deciles,
            });
        }

        out.push(VariableStats {
            name: variable.clone(),
            n,
            total: designs.len(),
            min: sorted[0],
            max: sorted[n - 1],
            mean,
            std,
            median,
            parameters: parameter_stats,
        });
    }
    Ok(out)
}

/// Mean and sample standard deviation (n - 1 denominator; 0.0 for n < 2).
fn mean_std(xs: &[f64]) -> MeanStd {
    let n = xs.len();
    if n == 0 {
        return MeanStd {
            count: 0,
            mean: 0.0,
            std: 0.0,
        };
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    let std = if n < 2 {
        0.0
    } else {
        let ss = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>();
        (ss / (n - 1) as f64).sqrt()
    };
    MeanStd {
        count: n,
        mean,
        std,
    }
}

/// Whether a series has (numerically) zero variance.
fn variance_is_zero(xs: &[f64]) -> bool {
    let min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (max - min) < 1e-12
}

/// Average ranks of a series (ties share the mean of their positions,
/// 1-based), the basis of the Spearman correlation.
fn ranks(xs: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..xs.len()).collect();
    order.sort_by(|&a, &b| xs[a].partial_cmp(&xs[b]).expect("values are finite"));
    let mut out = vec![0.0; xs.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && xs[order[j + 1]] == xs[order[i]] {
            j += 1;
        }
        let avg = (i + j + 2) as f64 / 2.0; // 1-based positions, averaged
        for &k in &order[i..=j] {
            out[k] = avg;
        }
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(name: &str, min: f64, max: f64) -> Parameter {
        Parameter::uniform(name, min, max).unwrap()
    }

    fn designs(values: &[(f64, f64)]) -> Vec<Design> {
        values
            .iter()
            .enumerate()
            .map(|(i, &(x, _))| Design {
                name: format!("0.{}", i + 1),
                values: vec![x],
            })
            .collect()
    }

    fn results(names: &[&str], energy: &[f64]) -> Vec<Results> {
        names
            .iter()
            .zip(energy)
            .map(|(name, &e)| Results {
                name: (*name).to_owned(),
                values: BTreeMap::from([("energy".to_owned(), e)]),
            })
            .collect()
    }

    #[test]
    fn rejects_empty_results() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0)]);
        assert!(analyze(&params, &designs, &[]).is_err());
    }

    #[test]
    fn rejects_unknown_result_name() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0)]);
        let results = results(&["0.1", "9.9"], &[1.0, 2.0]);
        let err = analyze(&params, &designs, &results).unwrap_err();
        assert!(err.to_string().contains("does not match any design"));
    }

    #[test]
    fn rejects_non_numeric_only_results() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0)]);
        let empty = Results {
            name: "0.1".to_owned(),
            values: BTreeMap::new(),
        };
        let err = analyze(&params, &designs, &[empty]).unwrap_err();
        assert!(err.to_string().contains("no numeric values"));
    }

    #[test]
    fn descriptive_statistics_are_hand_computed() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0), (5.0, 0.0)]);
        let results = results(
            &["0.1", "0.2", "0.3", "0.4", "0.5"],
            &[1.0, 2.0, 3.0, 4.0, 5.0],
        );
        let stats = analyze(&params, &designs, &results).unwrap();
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.name, "energy");
        assert_eq!(s.n, 5);
        assert_eq!(s.total, 5);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 5.0);
        assert!((s.mean - 3.0).abs() < 1e-12);
        // sample std of 1..=5 is sqrt(2.5)
        assert!((s.std - 2.5_f64.sqrt()).abs() < 1e-12);
        assert_eq!(s.median, 3.0);
    }

    #[test]
    fn median_of_even_n_is_the_midpoint() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]);
        let results = results(&["0.1", "0.2", "0.3", "0.4"], &[1.0, 2.0, 3.0, 10.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        assert_eq!(stats[0].median, 2.5);
    }

    #[test]
    fn correlations_are_none_below_three_points() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0)]);
        let results = results(&["0.1", "0.2"], &[1.0, 2.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        assert_eq!(stats[0].parameters[0].pearson, None);
        assert_eq!(stats[0].parameters[0].spearman, None);
    }

    #[test]
    fn correlations_are_none_for_zero_variance() {
        let params = vec![param("x", 0.0, 10.0)];
        // all designs share x = 5.0: no variance in the parameter series
        let designs = designs(&[(5.0, 0.0), (5.0, 0.0), (5.0, 0.0)]);
        let results = results(&["0.1", "0.2", "0.3"], &[1.0, 2.0, 3.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        assert_eq!(stats[0].parameters[0].pearson, None);
        assert_eq!(stats[0].parameters[0].spearman, None);
    }

    #[test]
    fn linear_trend_gives_perfect_correlations() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]);
        // energy = 2x + 1
        let results = results(&["0.1", "0.2", "0.3", "0.4"], &[3.0, 5.0, 7.0, 9.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        let p = &stats[0].parameters[0];
        assert!((p.pearson.unwrap() - 1.0).abs() < 1e-12);
        assert!((p.spearman.unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn spearman_catches_monotonic_nonlinear_trends() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]);
        // energy = x^2: monotonic, but not linear
        let results = results(&["0.1", "0.2", "0.3", "0.4"], &[1.0, 4.0, 9.0, 16.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        let p = &stats[0].parameters[0];
        assert!((p.spearman.unwrap() - 1.0).abs() < 1e-12);
        assert!(p.pearson.unwrap() < 0.99, "pearson should be < 1");
    }

    #[test]
    fn ranks_average_ties() {
        assert_eq!(ranks(&[1.0, 1.0, 2.0]), vec![1.5, 1.5, 3.0]);
        assert_eq!(ranks(&[3.0, 1.0, 1.0]), vec![3.0, 1.5, 1.5]);
        assert_eq!(ranks(&[10.0]), vec![1.0]);
    }

    #[test]
    fn scope_split_and_deciles_are_correct() {
        let params = vec![
            Parameter::uniform("x", 0.0, 10.0)
                .unwrap()
                .with_scope(2.0, 8.0)
                .unwrap(),
        ];
        // x = 0.5 lies outside the scope, 5.0 and 9.5... 9.5 is also outside
        let designs = designs(&[(0.5, 0.0), (5.0, 0.0), (9.5, 0.0)]);
        let results = results(&["0.1", "0.2", "0.3"], &[1.0, 2.0, 3.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        let p = &stats[0].parameters[0];

        assert_eq!(p.in_scope.count, 1);
        assert_eq!(p.in_scope.mean, 2.0);
        let out = p.out_of_scope.as_ref().expect("out of scope set");
        assert_eq!(out.count, 2);
        assert!((out.mean - 2.0).abs() < 1e-12);

        // deciles: bin 0 holds x=0.5 (result 1.0), bin 5 holds x=5.0
        // (result 2.0), bin 9 holds x=9.5 (result 3.0)
        assert_eq!(p.deciles.len(), DECILES);
        assert_eq!(
            p.deciles[0],
            Decile {
                count: 1,
                mean: Some(1.0)
            }
        );
        assert_eq!(
            p.deciles[4],
            Decile {
                count: 0,
                mean: None
            }
        );
        assert_eq!(
            p.deciles[5],
            Decile {
                count: 1,
                mean: Some(2.0)
            }
        );
        assert_eq!(
            p.deciles[9],
            Decile {
                count: 1,
                mean: Some(3.0)
            }
        );
        assert_eq!(
            p.deciles[1],
            Decile {
                count: 0,
                mean: None
            }
        );
    }

    #[test]
    fn pool_analysis_rejects_an_empty_pool() {
        let params = vec![param("x", 0.0, 10.0)];
        let err = analyze_pool(&params, &[]).unwrap_err();
        assert!(err.to_string().contains("no designs to analyze"));
    }

    #[test]
    fn pool_analysis_rejects_designs_with_wrong_length() {
        let params = vec![param("x", 0.0, 10.0)];
        let bad = Design {
            name: "0.1".to_owned(),
            values: vec![1.0, 2.0],
        };
        assert!(analyze_pool(&params, &[bad]).is_err());
    }

    #[test]
    fn pool_analysis_describes_each_parameter() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs: Vec<Design> = [0.5, 1.5, 5.0, 9.9]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x],
            })
            .collect();
        let pool = analyze_pool(&params, &designs).unwrap();
        assert_eq!(pool.total, 4);
        let p = &pool.parameters[0];
        assert_eq!(p.name, "x");
        assert_eq!(p.n, 4);
        assert_eq!(p.min, 0.5);
        assert_eq!(p.max, 9.9);
        assert!((p.mean - 4.225).abs() < 1e-12);
        assert!((p.std - (54.1075_f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((p.median - 3.25).abs() < 1e-12);
        // one point per occupied decile of [0, 10]: bins 0, 1, 5, 9
        assert_eq!(p.deciles, vec![1, 1, 0, 0, 0, 1, 0, 0, 0, 1]);
        // the outer deciles (first + last) hold bins 0 and 9
        assert_eq!(p.outer_deciles, 2);
        // no pairs with a single parameter
        assert!(pool.pairs.is_empty());
    }

    #[test]
    fn pool_metrics_of_a_single_center_design() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = vec![Design {
            name: "0.1".to_owned(),
            values: vec![5.0],
        }];
        let pool = analyze_pool(&params, &designs).unwrap();
        // no pair of designs: distances are undefined
        assert_eq!(pool.min_pairwise_distance, None);
        assert_eq!(pool.mean_nearest_neighbor, None);
        // one point at the center of the unit cube: CL2 = sqrt(1/12)
        assert!((pool.discrepancy_cl2 - (1.0_f64 / 12.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn pool_metrics_of_two_designs_at_the_range_extremes() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = vec![
            Design {
                name: "0.1".to_owned(),
                values: vec![0.0],
            },
            Design {
                name: "0.2".to_owned(),
                values: vec![10.0],
            },
        ];
        let pool = analyze_pool(&params, &designs).unwrap();
        // normalized to u = 0 and u = 1: distance 1
        assert!((pool.min_pairwise_distance.unwrap() - 1.0).abs() < 1e-12);
        assert!((pool.mean_nearest_neighbor.unwrap() - 1.0).abs() < 1e-12);
        // two points at the extremes of [0, 1]: CL2 = sqrt(1/3)
        assert!((pool.discrepancy_cl2 - (1.0_f64 / 3.0).sqrt()).abs() < 1e-12);
        // both points sit in the outer deciles
        assert_eq!(pool.parameters[0].outer_deciles, 2);
    }

    #[test]
    fn pool_metrics_are_hand_computed_in_the_unit_cube() {
        let params = vec![param("x", 0.0, 10.0), param("y", 0.0, 10.0)];
        // normalized to u = (0, 0) and (0.3, 0.4): distance 0.5
        let designs = vec![
            Design {
                name: "0.1".to_owned(),
                values: vec![0.0, 0.0],
            },
            Design {
                name: "0.2".to_owned(),
                values: vec![3.0, 4.0],
            },
        ];
        let pool = analyze_pool(&params, &designs).unwrap();
        assert!((pool.min_pairwise_distance.unwrap() - 0.5).abs() < 1e-12);
        assert!((pool.mean_nearest_neighbor.unwrap() - 0.5).abs() < 1e-12);
        // CL2 = sqrt((13/12)^2 - 2*(1.25^2 + 1.16*1.09) + (4 + 2.17 + 2.17 + 1.68)/4)
        let expected = (169.0 / 144.0 - 2.8269 + 2.505_f64).sqrt();
        assert!((pool.discrepancy_cl2 - expected).abs() < 1e-12);
        // x = 0 and y = 0 lie in decile 0, the other design does not
        assert_eq!(pool.parameters[0].outer_deciles, 1);
        assert_eq!(pool.parameters[1].outer_deciles, 1);
    }

    #[test]
    fn nearest_neighbor_distances_of_three_points() {
        let params = vec![param("x", 0.0, 10.0)];
        // normalized to u = 0.1, 0.3, 0.6
        let designs: Vec<Design> = [1.0, 3.0, 6.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x],
            })
            .collect();
        let pool = analyze_pool(&params, &designs).unwrap();
        // pairwise distances: |0.1-0.3| = 0.2, |0.1-0.6| = 0.5, |0.3-0.6| = 0.3
        assert!((pool.min_pairwise_distance.unwrap() - 0.2).abs() < 1e-12);
        // nearest neighbors: 0.2, 0.2, 0.3 -> mean 0.7/3
        assert!((pool.mean_nearest_neighbor.unwrap() - 0.7 / 3.0).abs() < 1e-12);
        // bins 1, 3 and 6: nothing in the first or last decile
        assert_eq!(pool.parameters[0].outer_deciles, 0);
    }

    #[test]
    fn pool_analysis_correlates_parameter_pairs() {
        let params = vec![param("x", 0.0, 10.0), param("y", 0.0, 10.0)];
        // y = 2x: perfectly linear pair
        let linear: Vec<Design> = [1.0, 2.0, 3.0, 4.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x, 2.0 * x],
            })
            .collect();
        let pool = analyze_pool(&params, &linear).unwrap();
        assert_eq!(pool.pairs.len(), 1);
        let pair = &pool.pairs[0];
        assert_eq!(pair.a, "x");
        assert_eq!(pair.b, "y");
        assert!((pair.pearson.unwrap() - 1.0).abs() < 1e-12);
        assert!((pair.spearman.unwrap() - 1.0).abs() < 1e-12);

        // fewer than 3 designs: correlations are undefined
        let short: Vec<Design> = [1.0, 2.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x, 2.0 * x],
            })
            .collect();
        let pool = analyze_pool(&params, &short).unwrap();
        assert_eq!(pool.pairs[0].pearson, None);
        assert_eq!(pool.pairs[0].spearman, None);

        // a parameter without variance: correlations are undefined
        let flat: Vec<Design> = [5.0, 5.0, 5.0]
            .into_iter()
            .enumerate()
            .map(|(i, x)| Design {
                name: format!("0.{}", i + 1),
                values: vec![x, i as f64],
            })
            .collect();
        let pool = analyze_pool(&params, &flat).unwrap();
        assert_eq!(pool.pairs[0].pearson, None);
        assert_eq!(pool.pairs[0].spearman, None);
    }

    #[test]
    fn out_of_scope_is_none_when_everything_is_in_scope() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(5.0, 0.0), (6.0, 0.0), (7.0, 0.0)]);
        let results = results(&["0.1", "0.2", "0.3"], &[1.0, 2.0, 3.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        assert_eq!(stats[0].parameters[0].out_of_scope, None);
        assert_eq!(stats[0].parameters[0].in_scope.count, 3);
    }
}
