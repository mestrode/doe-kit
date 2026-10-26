//! The `sampling` command-line tool: Design of Experiments sampling methods.
//!
//! One subcommand per sampling method. `sampling plhs` is pooled Latin
//! Hypercube Sampling: it reads a JSON design file and adds new designs in
//! one of two modes — `--add N` refines inside the per-parameter scopes,
//! `--explore N` covers the whole surface (uniform over each parameter's
//! full definition range). The result is written back (same file by
//! default, with a `.bak` backup of the input unless disabled).
//!
//! `sampling init` writes a new pool file from the doe-pool skeleton
//! (two example parameters, no designs) and refuses to overwrite an
//! existing file.
//!
//! `sampling stats` analyzes the pool (read-only judgment support before
//! expensive simulations are run): statistics of the sampling points
//! themselves — per-parameter descriptive statistics, decile coverage and
//! outer-decile counts of the definition range, space-filling metrics in
//! the unit cube (min/mean/median pairwise distance, mean
//! nearest-neighbor distance, centered L2 discrepancy) and pairwise
//! parameter correlations — plus, when results exist, coverage,
//! per-parameter correlations, scope split and decile means per result
//! variable. The pooled pair r2 (the sum of squared pair correlations,
//! the plhs score) is readable back on the pool.
//!
//! Every subcommand that produces a result accepts `--json`: one line of
//! machine-readable JSON on stdout instead of the human output, so runs
//! can be collected line by line (benchmarks, result files). Errors stay
//! human on stderr with the usual exit codes; `--json` only changes the
//! success output.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "sampling",
    version,
    about = "Design of Experiments sampling methods; each method is a subcommand"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a new pool file with two example parameters and no designs;
    /// refuses to overwrite an existing file
    #[command(name = "init")]
    Init(InitArgs),
    /// Pooled Latin Hypercube Sampling: augment a design pool with
    /// low-correlation designs, scoped (--add) or whole-surface (--explore)
    #[command(name = "plhs")]
    Plhs(PlhsArgs),
    /// Statistical analysis of the pool: sampling-point statistics plus,
    /// when results exist, per-result coverage, correlations, scope
    /// split and decile means (read-only)
    #[command(name = "stats")]
    Stats(StatsArgs),
}

#[derive(Args)]
struct InitArgs {
    /// Output file; must not already exist.
    file: PathBuf,
}

#[derive(Args)]
struct PlhsArgs {
    /// Input JSON file; also the output file unless --output is given.
    file: PathBuf,
    /// Number of new designs inside the per-parameter scopes.
    #[arg(short, long)]
    add: Option<usize>,
    /// Number of new designs uniformly covering each parameter's full
    /// definition range (ignores scope and distribution).
    #[arg(long)]
    explore: Option<usize>,
    /// Random seed (fixed default for reproducible runs).
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Output file (default: the input file).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Skip writing the backup file.
    #[arg(long)]
    no_backup: bool,
    /// Maximum number of assignments enumerated exactly before falling back
    /// to greedy selection.
    #[arg(long, default_value_t = 1_000_000)]
    cap: u64,
    /// Skip the post-placement refinement pass (swap descent) that runs
    /// after the selection on greedy, mixed and batched runs.
    #[arg(long)]
    no_polish: bool,
    /// Print one line of machine-readable JSON instead of the human
    /// summary (for benchmarks and result collection).
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct StatsArgs {
    /// Input JSON file with the pool and evaluated results.
    file: PathBuf,
    /// Print one line of machine-readable JSON instead of the human
    /// report (for benchmarks and result collection).
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init(args) => run_init(&args),
        Command::Plhs(args) => run(&args),
        Command::Stats(args) => run_stats(&args),
    };
    match result {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            e.exit_code()
        }
    }
}

/// Errors mapped onto the exit-code policy: 1 for validation, 2 for I/O.
#[derive(Debug)]
enum CliError {
    Validation(String),
    Io(std::io::Error),
}

impl CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Validation(_) => ExitCode::from(1),
            Self::Io(_) => ExitCode::from(2),
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(msg) => write!(f, "error: {msg}"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

fn validation(e: plhs::Error) -> CliError {
    CliError::Validation(e.to_string())
}

fn pool(e: doe_pool::Error) -> CliError {
    match e {
        doe_pool::Error::Validation(e) => CliError::Validation(e.to_string()),
        doe_pool::Error::Io(e) => CliError::Io(e),
    }
}

/// Writes a new pool file from the doe-pool skeleton. Refuses to overwrite
/// an existing target (validation error, exit 1); other I/O failures exit
/// 2 as usual.
fn run_init(args: &InitArgs) -> Result<String, CliError> {
    if args.file.exists() {
        return Err(CliError::Validation(format!(
            "'{}' already exists",
            args.file.display()
        )));
    }
    let root = doe_pool::skeleton();
    let bytes = doe_pool::serialize(&root).map_err(pool)?;
    doe_pool::write_atomic(&args.file, &bytes).map_err(pool)?;
    Ok(format!(
        "initialized '{}' with 2 example parameters (edit 'params' before use)",
        args.file.display()
    ))
}

fn run(args: &PlhsArgs) -> Result<String, CliError> {
    // exactly one mode: scoped refinement or whole-surface exploration
    let (verb, m, explore) = match (args.add, args.explore) {
        (Some(n), None) => ("added", n, false),
        (None, Some(n)) => ("explored", n, true),
        (Some(_), Some(_)) => {
            return Err(CliError::Validation(
                "--add and --explore are mutually exclusive; give exactly one".to_owned(),
            ));
        }
        (None, None) => {
            return Err(CliError::Validation(
                "one of --add or --explore is required".to_owned(),
            ));
        }
    };
    if m == 0 {
        return Err(CliError::Validation(format!(
            "{mode} must be at least 1",
            mode = if explore { "--explore" } else { "--add" }
        )));
    }
    let (bytes, mut root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let started = std::time::Instant::now();
    let polish = !args.no_polish;
    let aug = if explore {
        plhs::explore(&params, &designs, m, args.seed, args.cap, polish).map_err(validation)?
    } else {
        plhs::augment(&params, &designs, m, args.seed, args.cap, polish).map_err(validation)?
    };

    if !args.no_backup {
        doe_pool::backup(&args.file, &bytes).map_err(pool)?;
    }

    doe_pool::format::append_new_designs(&mut root, &params, &aug.new_designs)
        .map_err(validation)?;
    let out_bytes = doe_pool::serialize(&root).map_err(pool)?;
    let out = args.output.clone().unwrap_or_else(|| args.file.clone());
    doe_pool::write_atomic(&out, &out_bytes).map_err(pool)?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    if args.json {
        let mut report = serde_json::json!({
            "subcommand": "plhs",
            "mode": if explore { "explore" } else { "add" },
            "file": args.file.display().to_string(),
            "seed": args.seed,
            "cap": args.cap,
            "m": m,
            "designs_before": designs.len(),
            "designs_after": aug.new_designs.len() + designs.len(),
            "iteration": aug.iteration,
            "n_params": params.len(),
            "strategy": aug.strategy.name(),
            "chunks": aug.chunks,
            "polished": aug.polish.is_some(),
            "score": aug.score,
            "elapsed_ms": elapsed_ms,
        });
        // the pass reports what it did; without it there is nothing to
        // report (skipped or disabled)
        if let Some(p) = &aug.polish {
            report["polish_moves"] = serde_json::json!(p.moves);
            report["polish_score_before"] = serde_json::json!(p.score_before);
            report["polish_score_after"] = serde_json::json!(p.score_after);
        }
        return Ok(report.to_string());
    }

    let mut summary = format!(
        "{verb} {} designs (iteration {}), score {:.6}",
        aug.new_designs.len(),
        aug.iteration,
        aug.score
    );
    if let Some(p) = &aug.polish {
        if p.moves > 0 {
            summary.push_str(&format!(
                ", polished with {} swap(s) (was {:.6})",
                p.moves, p.score_before
            ));
        }
    }
    Ok(summary)
}

/// Analyzes the pool and formats the statistics report: statistics of the
/// sampling points themselves, plus the evaluated results when any exist.
/// Read-only: no files are written.
fn run_stats(args: &StatsArgs) -> Result<String, CliError> {
    let (_, root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let results = doe_pool::format::parse_results(&root).map_err(validation)?;
    // results first: a result name matching no design keeps failing
    // exactly as before; the sampling points are reported on top
    let stats = if results.is_empty() {
        Vec::new()
    } else {
        doe_stats::analyze(&params, &designs, &results).map_err(validation)?
    };
    let pool = doe_stats::analyze_pool(&params, &designs).map_err(validation)?;
    if args.json {
        let report = serde_json::json!({
            "subcommand": "stats",
            "file": args.file.display().to_string(),
            "evaluated": results.len(),
            "pool": serde_json::to_value(&pool)
                .map_err(|e| validation(plhs::Error::Invalid(e.to_string())))?,
            "results": serde_json::to_value(&stats)
                .map_err(|e| validation(plhs::Error::Invalid(e.to_string())))?,
        });
        return Ok(report.to_string());
    }
    Ok(format_stats_report(&pool, &stats, results.len()))
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

/// Formats the statistics as a human-readable report: one block for the
/// sampling points, then one block per result variable. Statistics only,
/// the judgment stays with the reader.
fn format_stats_report(
    pool: &doe_stats::PoolStats,
    stats: &[doe_stats::VariableStats],
    evaluated: usize,
) -> String {
    let mut out = String::new();
    let parameters = pool.parameters.len();
    out.push_str(&format!(
        "pool: {} designs, {evaluated} evaluated, {parameters} parameters ({} designs per parameter)\n",
        pool.total,
        designs_per_parameter(pool.total, parameters)
    ));
    out.push_str("sampling points:\n");
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
    for v in stats {
        out.push_str(&format!(
            "{}: n={}, min {:.6}, max {:.6}, mean {:.6}, median {:.6}, std {:.6}\n",
            v.name, v.n, v.min, v.max, v.mean, v.median, v.std
        ));
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
    out.trim_end().to_owned()
}
