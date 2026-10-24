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

use plhs::error::{Error, Result};
use plhs::scoring::pearson;
use plhs::{Design, Parameter};

/// One evaluated design: the numeric fields of its result entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// Design name; must match a design in the pool.
    pub name: String,
    /// Numeric result variables (e.g. `energy: 42.1`). Non-numeric fields
    /// are the caller's business and never reach this type.
    pub values: BTreeMap<String, f64>,
}

/// Descriptive statistics of one result variable.
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct MeanStd {
    /// Number of designs in the set.
    pub count: usize,
    pub mean: f64,
    pub std: f64,
}

/// One decile bin of the definition range.
#[derive(Debug, Clone, PartialEq)]
pub struct Decile {
    /// Evaluated designs in this decile.
    pub count: usize,
    /// Mean result over them; `None` for an empty decile.
    pub mean: Option<f64>,
}

/// Number of deciles per parameter definition range.
pub const DECILES: usize = 10;

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
    fn out_of_scope_is_none_when_everything_is_in_scope() {
        let params = vec![param("x", 0.0, 10.0)];
        let designs = designs(&[(5.0, 0.0), (6.0, 0.0), (7.0, 0.0)]);
        let results = results(&["0.1", "0.2", "0.3"], &[1.0, 2.0, 3.0]);
        let stats = analyze(&params, &designs, &results).unwrap();
        assert_eq!(stats[0].parameters[0].out_of_scope, None);
        assert_eq!(stats[0].parameters[0].in_scope.count, 3);
    }
}
