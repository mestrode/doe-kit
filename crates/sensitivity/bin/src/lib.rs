//! Presentation of Design of Experiments pool statistics: the
//! human-readable report and the machine-readable JSON line.
//!
//! Pure formatting: the statistics themselves are computed by the
//! `doe-stats` library crate; this crate never computes and never writes
//! files. It is the shared front-end library for the `doe-stats` binary
//! (subcommands `input`, `sampling`, `output` plus the bare full report)
//! and serves the `sampling stats` subcommand in the meantime.

use doe_stats::{DECILES, PoolStats, VariableStats};

/// Formats the statistics as a human-readable report: one block for the
/// sampling points, then one block per result variable. Statistics only,
/// the judgment stays with the reader. The default is the compact tier
/// (global metrics and summary lines); `verbose` adds the per-parameter
/// descriptive statistics and decile counts, the complete pair list and
/// the per-parameter blocks of the result variables.
pub fn human_report(
    pool: &PoolStats,
    results: &[VariableStats],
    evaluated: usize,
    verbose: bool,
) -> String {
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
    let max_of = |v: Option<f64>| match v {
        Some(x) => format!("{x:.6}"),
        None => "n/a".to_owned(),
    };
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
    for v in results {
        out.push_str(&format!(
            "{}: n={}, min {:.6}, max {:.6}, mean {:.6}, median {:.6}, std {:.6}\n",
            v.name, v.n, v.min, v.max, v.mean, v.median, v.std
        ));
        if verbose {
            for p in &v.parameters {
                match (p.pearson, p.spearman) {
                    (Some(pe), Some(se)) => {
                        out.push_str(&format!(
                            "  {}: pearson {pe:.6}, spearman {se:.6}\n",
                            p.name
                        ));
                    }
                    _ => {
                        out.push_str(&format!(
                            "  {}: pearson n/a, spearman n/a (n < 3 or no variance)\n",
                            p.name
                        ));
                    }
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
    out.trim_end().to_owned()
}

/// Builds the one-line machine-readable JSON report: `subcommand` (the
/// front-end's own label), `file`, `evaluated`, the pool statistics and
/// the result statistics. The default is the compact tier — the
/// per-parameter details and the pair list are dropped, the pool keeps
/// its summary fields and an `n_pairs` count; `verbose` keeps the full
/// objects. Serialization failures are reported, everything else cannot
/// fail (the statistics structs serialize to objects by construction).
pub fn json_report(
    subcommand: &str,
    file: &str,
    evaluated: usize,
    pool: &PoolStats,
    results: &[VariableStats],
    verbose: bool,
) -> Result<String, serde_json::Error> {
    let mut pool_v = serde_json::to_value(pool)?;
    let mut results_v = serde_json::to_value(results)?;
    if !verbose {
        // compact tier: global metrics and summary fields only —
        // the per-parameter details and the pair list are dropped
        let n_pairs = pool_v["pairs"].as_array().map_or(0, Vec::len);
        let obj = pool_v
            .as_object_mut()
            .expect("PoolStats serializes to an object");
        obj.remove("parameters");
        obj.remove("pairs");
        obj.insert("n_pairs".to_owned(), serde_json::json!(n_pairs));
        if let Some(results) = results_v.as_array_mut() {
            for entry in results {
                entry
                    .as_object_mut()
                    .expect("VariableStats serializes to an object")
                    .remove("parameters");
            }
        }
    }
    let report = serde_json::json!({
        "subcommand": subcommand,
        "file": file,
        "evaluated": evaluated,
        "pool": pool_v,
        "results": results_v,
    });
    Ok(report.to_string())
}

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
