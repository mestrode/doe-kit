//! Presentation of Design of Experiments pool statistics: the
//! human-readable report and the machine-readable JSON line.
//!
//! Pure formatting: the statistics themselves are computed by the
//! `doe-stats` library crate; this crate never computes and never writes
//! files. It is the shared front-end library of the `doe-stats` binary
//! (subcommands `input`, `sampling`, `output` plus the bare full report)
//! and serves the `sampling stats` subcommand in the meantime.
//!
//! The report is organized in three sections, one per view:
//!
//! - input: the parameter definitions (distribution, definition range,
//!   the Gaussian's mean and sigma, the scope with its share of the
//!   range). Single tier: definitions are always printed in full.
//! - sampling: the statistics of the sampling points — space-filling
//!   metrics in the unit cube, occupancy and pair summaries.
//! - output: the statistics of the evaluated results plus the pair
//!   correlations between the result variables.
//!
//! Sections are strict: the caller computes what its view needs, and a
//! view whose data is absent fails before formatting. The default tier
//! is compact; `--verbose` switches to the full detail — for the human
//! report and `--json` alike.

use doe_stats::{
    DECILES, InputStats, NamedValue, ParameterDefinition, PoolStats, ResultPairStats, VariableStats,
};

// ---------------------------------------------------------------------
// human report
// ---------------------------------------------------------------------

/// Formats the input section: one line per parameter definition.
/// Single tier — the definitions are always printed in full.
/// Total lost probability above which the compact input section warns
/// (fraction of the untruncated Gaussian mass outside the scope).
const LOST_PROBABILITY_CRITICAL: f64 = 0.10;

/// Formats the input section: one line per parameter definition. The
/// default is compact: definitions only, plus a warning under a
/// Gaussian parameter whose untruncated mass outside the scope
/// exceeds the critical fraction. `verbose` shows the lost-probability
/// values of every Gaussian parameter instead of the warning.
pub fn human_input(input: &InputStats, verbose: bool) -> String {
    let mut out = format!("input parameters ({}):\n", input.parameters.len());
    for p in &input.parameters {
        out.push_str(&format!("  {}: {}\n", p.name, definition_line(p)));
        if let Some(lp) = &p.lost_probability {
            if verbose {
                out.push_str(&format!(
                    "    lost probability: {:.1}% outside the range ({:.1}% below, {:.1}% above)\n",
                    (lp.below_range + lp.above_range) * 100.0,
                    lp.below_range * 100.0,
                    lp.above_range * 100.0
                ));
                if let (Some(below), Some(above)) = (lp.below_scope, lp.above_scope) {
                    out.push_str(&format!(
                        "    lost probability: {:.1}% outside the scope ({:.1}% below, {:.1}% above)\n",
                        (below + above) * 100.0,
                        below * 100.0,
                        above * 100.0
                    ));
                }
            } else {
                // the warning tracks the scope: what sampling actually
                // covers; a full-range scope equals the range figures
                let (below, above) = match (lp.below_scope, lp.above_scope) {
                    (Some(below), Some(above)) => (below, above),
                    _ => (lp.below_range, lp.above_range),
                };
                if below + above > LOST_PROBABILITY_CRITICAL {
                    out.push_str(&format!(
                        "    warning: {:.1}% of the probability mass lies outside the scope\n",
                        (below + above) * 100.0
                    ));
                }
            }
        }
    }
    out
}

/// One parameter definition as a report line fragment (without name).
fn definition_line(p: &ParameterDefinition) -> String {
    let mut s = format!("{}, range [{}, {}]", p.distribution, p.min, p.max);
    if let (Some(mean), Some(sigma)) = (p.mean, p.sigma) {
        s.push_str(&format!(", mean {mean}, sigma {sigma}"));
    }
    match p.scope {
        Some((lo, hi)) => s.push_str(&format!(
            ", scope [{lo}, {hi}] ({:.1}% of range)",
            p.scope_fraction * 100.0
        )),
        None => s.push_str(", scope: full range"),
    }
    s
}

/// Formats the sampling section: the pool line and the sampling-point
/// statistics. The default is the compact tier (global metrics and the
/// occupancy and pair summaries); `verbose` adds the per-parameter
/// descriptive statistics and decile counts and the complete pair list.
pub fn human_sampling(pool: &PoolStats, evaluated: usize, verbose: bool) -> String {
    let mut out = String::new();
    let parameters = pool.parameters.len();
    out.push_str(&format!(
        "pool: {} designs, {evaluated} evaluated, {parameters} parameters ({} designs per parameter)\n",
        pool.total,
        designs_per_parameter(pool.total, parameters)
    ));
    out.push_str("sampling points:\n");
    if verbose {
        for p in &pool.parameters {
            out.push_str(&format!(
                "  {}: n={}, min {:.6}, max {:.6}, mean {:.6}, median {:.6}, std {:.6}\n",
                p.name, p.n, p.min, p.max, p.mean, p.median, p.std
            ));
            let deciles: Vec<String> = p.deciles.iter().map(|c| c.to_string()).collect();
            out.push_str(&format!(
                "     decile counts: [{}], outer deciles: {}\n",
                deciles.join(", "),
                p.outer_deciles
            ));
        }
    }
    match pool.min_pairwise_distance {
        Some(d) => out.push_str(&format!("  min pairwise distance: {d:.6}\n")),
        None => out.push_str("  min pairwise distance: n/a (single design)\n"),
    }
    match pool.mean_pairwise_distance {
        Some(d) => out.push_str(&format!("  mean pairwise distance: {d:.6}\n")),
        None => out.push_str("  mean pairwise distance: n/a (single design)\n"),
    }
    match pool.median_pairwise_distance {
        Some(d) => out.push_str(&format!("  median pairwise distance: {d:.6}\n")),
        None => out.push_str("  median pairwise distance: n/a (single design)\n"),
    }
    match pool.mean_nearest_neighbor {
        Some(d) => out.push_str(&format!("  mean nearest neighbor: {d:.6}\n")),
        None => out.push_str("  mean nearest neighbor: n/a (single design)\n"),
    }
    out.push_str(&format!(
        "  discrepancy (CL2): {:.6}\n",
        pool.discrepancy_cl2
    ));
    match pool.pooled_pair_r2 {
        Some(s) => out.push_str(&format!("  pooled pair r2: {s:.6}\n")),
        None => out.push_str("  pooled pair r2: n/a (n < 3, single parameter or no variance)\n"),
    }
    // compact occupancy and pair summaries — readable in the default
    // tier, where the per-parameter decile counts and the pair list
    // are not shown
    out.push_str(&format!(
        "  empty decile slots: {} of {}\n",
        pool.empty_decile_slots,
        parameters * DECILES
    ));
    out.push_str(&format!(
        "  pairs: {}, max |pearson| {}, max |spearman| {}\n",
        pool.pairs.len(),
        max_of(pool.max_abs_pearson),
        max_of(pool.max_abs_spearman)
    ));
    if verbose {
        for pair in &pool.pairs {
            match (pair.pearson, pair.spearman) {
                (Some(pe), Some(se)) => out.push_str(&format!(
                    "  {} vs {}: pearson {pe:.6}, spearman {se:.6}\n",
                    pair.a, pair.b
                )),
                _ => out.push_str(&format!(
                    "  {} vs {}: pearson n/a, spearman n/a (n < 3 or no variance)\n",
                    pair.a, pair.b
                )),
            }
        }
    }
    out
}

/// Formats the output section: one block per result variable, then the
/// pair correlations between the result variables. The compact tier
/// prints one summary line per variable and the output-pairs summary;
/// `verbose` adds the per-parameter blocks and the complete pair list.
/// With fewer than two result variables the pairs block is omitted.
pub fn human_output(results: &[VariableStats], pairs: &ResultPairStats, verbose: bool) -> String {
    let mut out = String::from("outputs:\n");
    for v in results {
        out.push_str(&format!(
            "{}: n={}, min {:.6}, max {:.6}, mean {:.6}, median {:.6}, std {:.6}, min at {}, max at {}\n",
            v.name,
            v.n,
            v.min,
            v.max,
            v.mean,
            v.median,
            v.std,
            v.extremes.min_design,
            v.extremes.max_design
        ));
        // the strongest input→output relations, the compact summary of
        // the per-parameter blocks (always printed; n/a per metric when
        // no parameter defines it)
        out.push_str(&format!(
            "  max |pearson| {}, max |spearman| {}, max eta2 {}\n",
            relation_of(&v.relations.max_abs_pearson),
            relation_of(&v.relations.max_abs_spearman),
            relation_of(&v.relations.max_eta2),
        ));
        if verbose {
            for p in &v.parameters {
                let pearson = p
                    .pearson
                    .map(|r| format!("{r:.6}"))
                    .unwrap_or_else(|| "n/a".to_owned());
                let spearman = p
                    .spearman
                    .map(|r| format!("{r:.6}"))
                    .unwrap_or_else(|| "n/a".to_owned());
                let eta2 = p
                    .eta2
                    .map(|e| format!("{e:.6}"))
                    .unwrap_or_else(|| "n/a".to_owned());
                if p.pearson.is_some() && p.spearman.is_some() && p.eta2.is_some() {
                    out.push_str(&format!(
                        "  {}: pearson {pearson}, spearman {spearman}, eta2 {eta2}\n",
                        p.name
                    ));
                } else {
                    out.push_str(&format!(
                        "  {}: pearson {pearson}, spearman {spearman}, eta2 {eta2} (n < 3, no variance or too few occupied deciles)\n",
                        p.name
                    ));
                }
                out.push_str(&format!(
                    "     in scope: {} evaluated, mean {:.6}, std {:.6}\n",
                    p.in_scope.count, p.in_scope.mean, p.in_scope.std
                ));
                match &p.out_of_scope {
                    Some(o) => out.push_str(&format!(
                        "     outside scope: {} evaluated, mean {:.6}, std {:.6}\n",
                        o.count, o.mean, o.std
                    )),
                    None => out.push_str("     outside scope: none evaluated\n"),
                }
                let deciles: Vec<String> = p
                    .deciles
                    .iter()
                    .map(|d| match d.mean {
                        Some(m) => format!("{m:.6}"),
                        None => "-".to_owned(),
                    })
                    .collect();
                out.push_str(&format!("     deciles: [{}]\n", deciles.join(", ")));
            }
        }
    }
    if !pairs.pairs.is_empty() {
        out.push_str(&format!(
            "output pairs: {}, max |pearson| {}, max |spearman| {}\n",
            pairs.pairs.len(),
            max_of(pairs.max_abs_pearson),
            max_of(pairs.max_abs_spearman)
        ));
        if verbose {
            for pair in &pairs.pairs {
                match (pair.pearson, pair.spearman) {
                    (Some(pe), Some(se)) => out.push_str(&format!(
                        "  {} vs {}: pearson {pe:.6}, spearman {se:.6} (n {})\n",
                        pair.a, pair.b, pair.n
                    )),
                    _ => out.push_str(&format!(
                        "  {} vs {}: pearson n/a, spearman n/a (n {}; n < 3 or no variance)\n",
                        pair.a, pair.b, pair.n
                    )),
                }
            }
        }
    }
    out
}

/// The computed sections of a full report: the input definitions, the
/// sampling statistics, the per-result statistics and the output
/// correlations. The caller has already run all analyses (a section
/// that cannot run fails before the report is formatted).
pub struct ReportData<'a> {
    pub input: &'a InputStats,
    pub pool: &'a PoolStats,
    pub results: &'a [VariableStats],
    pub pairs: &'a ResultPairStats,
    pub evaluated: usize,
}

/// Formats the full report: the input section, then the sampling
/// section, then the output section.
pub fn human_report(data: &ReportData, verbose: bool) -> String {
    let mut out = String::new();
    out.push_str(&human_input(data.input, verbose));
    out.push_str(&human_sampling(data.pool, data.evaluated, verbose));
    out.push_str(&human_output(data.results, data.pairs, verbose));
    out.trim_end().to_owned()
}

// ---------------------------------------------------------------------
// JSON report
// ---------------------------------------------------------------------

/// The pool statistics as a JSON value; the compact tier keeps the
/// global metrics and summary fields and replaces the per-parameter
/// details and the pair list with an `n_pairs` count.
fn pool_value(pool: &PoolStats, verbose: bool) -> Result<serde_json::Value, serde_json::Error> {
    let mut v = serde_json::to_value(pool)?;
    if !verbose {
        let n_pairs = v["pairs"].as_array().map_or(0, Vec::len);
        let obj = v
            .as_object_mut()
            .expect("PoolStats serializes to an object");
        obj.remove("parameters");
        obj.remove("pairs");
        obj.insert("n_pairs".to_owned(), serde_json::json!(n_pairs));
    }
    Ok(v)
}

/// The result statistics as a JSON value; the compact tier drops the
/// per-parameter blocks of each variable.
fn results_value(
    results: &[VariableStats],
    verbose: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut v = serde_json::to_value(results)?;
    if !verbose {
        if let Some(results) = v.as_array_mut() {
            for entry in results {
                entry
                    .as_object_mut()
                    .expect("VariableStats serializes to an object")
                    .remove("parameters");
            }
        }
    }
    Ok(v)
}

/// The output-pair statistics as a JSON value; the compact tier
/// replaces the pair list with an `n_pairs` count, keeping the maxima.
fn pairs_value(
    pairs: &ResultPairStats,
    verbose: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut v = serde_json::to_value(pairs)?;
    if !verbose {
        let n_pairs = v["pairs"].as_array().map_or(0, Vec::len);
        let obj = v
            .as_object_mut()
            .expect("ResultPairStats serializes to an object");
        obj.remove("pairs");
        obj.insert("n_pairs".to_owned(), serde_json::json!(n_pairs));
    }
    Ok(v)
}

/// Builds the one-line machine-readable JSON payload of the input view:
/// `subcommand` (the front-end's own label), `file` and the `input`
/// object. Single tier — the definitions have no detail depth.
pub fn json_input(
    subcommand: &str,
    file: &str,
    input: &InputStats,
) -> Result<String, serde_json::Error> {
    let report = serde_json::json!({
        "subcommand": subcommand,
        "file": file,
        "input": serde_json::to_value(input)?,
    });
    Ok(report.to_string())
}

/// Builds the one-line machine-readable JSON payload of the sampling
/// view: `subcommand`, `file`, `evaluated` and the `pool` object
/// (compact by default, full with `verbose`).
pub fn json_sampling(
    subcommand: &str,
    file: &str,
    evaluated: usize,
    pool: &PoolStats,
    verbose: bool,
) -> Result<String, serde_json::Error> {
    let report = serde_json::json!({
        "subcommand": subcommand,
        "file": file,
        "evaluated": evaluated,
        "pool": pool_value(pool, verbose)?,
    });
    Ok(report.to_string())
}

/// Builds the one-line machine-readable JSON payload of the output
/// view: `subcommand`, `file`, `evaluated`, the `results` array and the
/// `output_pairs` object (compact by default, full with `verbose`).
pub fn json_output(
    subcommand: &str,
    file: &str,
    evaluated: usize,
    results: &[VariableStats],
    pairs: &ResultPairStats,
    verbose: bool,
) -> Result<String, serde_json::Error> {
    let report = serde_json::json!({
        "subcommand": subcommand,
        "file": file,
        "evaluated": evaluated,
        "results": results_value(results, verbose)?,
        "output_pairs": pairs_value(pairs, verbose)?,
    });
    Ok(report.to_string())
}

/// Builds the one-line machine-readable JSON payload of the full
/// report: `subcommand`, `file`, `evaluated` and the three section
/// objects `input`, `pool`, `results` and `output_pairs` (the input
/// object is single tier; the others follow the tiering).
pub fn json_report(
    subcommand: &str,
    file: &str,
    data: &ReportData,
    verbose: bool,
) -> Result<String, serde_json::Error> {
    let report = serde_json::json!({
        "subcommand": subcommand,
        "file": file,
        "evaluated": data.evaluated,
        "input": serde_json::to_value(data.input)?,
        "pool": pool_value(data.pool, verbose)?,
        "results": results_value(data.results, verbose)?,
        "output_pairs": pairs_value(data.pairs, verbose)?,
    });
    Ok(report.to_string())
}

// ---------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------

/// Formats `n / k` as a context number: an integer when it divides
/// evenly, one decimal otherwise.
fn designs_per_parameter(designs: usize, parameters: usize) -> String {
    if parameters == 0 {
        return "0".to_owned();
    }
    if designs % parameters == 0 {
        format!("{}", designs / parameters)
    } else {
        format!("{:.1}", designs as f64 / parameters as f64)
    }
}

/// Formats a maximum of absolute correlations: `n/a` when none is
/// defined.
fn max_of(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.6}"),
        None => "n/a".to_owned(),
    }
}

/// Formats one relation maximum: `X (param)`, or `n/a` when no
/// parameter defines it.
fn relation_of(v: &Option<NamedValue>) -> String {
    match v {
        Some(nv) => format!("{:.6} ({})", nv.value, nv.parameter),
        None => "n/a".to_owned(),
    }
}
