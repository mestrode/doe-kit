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
//! (three example parameters `a`, `b`, `c`, no designs) and refuses to
//! overwrite an existing file.
//!
//! `sampling stats` analyzes the pool (read-only judgment support before
//! expensive simulations are run): the full report of the `doe-stats`
//! binary — the parameter definitions, then the statistics of the
//! sampling points themselves — space-filling metrics in the unit cube
//! (min/mean/median pairwise distance, mean nearest-neighbor distance,
//! centered L2 discrepancy, the pooled pair r2 that plhs optimizes) plus
//! the occupancy and pair summaries — then one summary line per result
//! variable and the pairs between the result variables. The sections
//! run in order and the first failing one ends the run (a pool without
//! designs fails at the sampling section, a pool without results at
//! the output section). `--verbose` switches to the full tier:
//! per-parameter descriptive statistics, decile coverage and
//! outer-decile counts, the complete pair lists and the per-parameter
//! result blocks.
//!
//! Every subcommand that produces a result accepts `--json`: one line of
//! machine-readable JSON on stdout instead of the human output, so runs
//! can be collected line by line (benchmarks, result files). Errors stay
//! human on stderr with the usual exit codes; `--json` only changes the
//! success output. Subcommands with detail depth follow one tiering:
//! the default output is compact, `--verbose` switches to full detail —
//! for the human report and for `--json` alike.

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
    /// Write a new pool file with three example parameters (`a`, `b`
    /// uniform, `c` gaussian) and no designs; refuses to overwrite an
    /// existing file
    #[command(name = "init")]
    Init(InitArgs),
    /// Pooled Latin Hypercube Sampling: augment a design pool with
    /// low-correlation designs, scoped (--add) or whole-surface (--explore)
    #[command(name = "plhs")]
    Plhs(PlhsArgs),
    /// Full statistics report of the pool: parameter definitions,
    /// sampling points and evaluated results (read-only; the
    /// `doe-stats` binary is the stats front-end, this subcommand is
    /// the transitional wrapper)
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
    /// Switch to the full detail tier: per-parameter descriptive
    /// statistics and decile counts, the complete pair list and the
    /// per-parameter blocks of the result variables. The default
    /// output is compact; applies to the human report and `--json`
    /// alike.
    #[arg(long)]
    verbose: bool,
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
    let n_params = root["params"].as_array().map_or(0, Vec::len);
    let bytes = doe_pool::serialize(&root).map_err(pool)?;
    doe_pool::write_atomic(&args.file, &bytes).map_err(pool)?;
    Ok(format!(
        "initialized '{}' with {n_params} example parameters (edit 'params' before use)",
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

/// Analyzes the pool and formats the full statistics report (the same
/// report as bare `doe-stats`): the parameter definitions, then the
/// sampling points, then the evaluated results. The sections run in
/// order and the first failing one ends the run. Read-only: no files
/// are written.
fn run_stats(args: &StatsArgs) -> Result<String, CliError> {
    let (_, root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let results = doe_pool::format::parse_results(&root).map_err(validation)?;
    // input section (definitions cannot fail), then the sampling
    // section (needs designs), then the output section (needs results)
    let input = doe_stats::analyze_inputs(&params);
    let pool = doe_stats::analyze_pool(&params, &designs).map_err(validation)?;
    if results.is_empty() {
        return Err(CliError::Validation("no results to analyze".to_owned()));
    }
    let stats = doe_stats::analyze(&params, &designs, &results).map_err(validation)?;
    let pairs = doe_stats::analyze_result_pairs(&designs, &results).map_err(validation)?;
    let data = doe_stats_bin::ReportData {
        input: &input,
        pool: &pool,
        results: &stats,
        pairs: &pairs,
        evaluated: results.len(),
    };
    if args.json {
        return doe_stats_bin::json_report(
            "stats",
            &args.file.display().to_string(),
            &data,
            args.verbose,
        )
        .map_err(|e| validation(plhs::Error::Invalid(e.to_string())));
    }
    Ok(doe_stats_bin::human_report(&data, args.verbose))
}
