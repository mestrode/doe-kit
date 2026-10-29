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
//! `sampling reset` removes designs from the pool: every design without
//! a results entry (the unevaluated ones), or — with `--force` — every
//! design together with the whole `results` array, returning the pool
//! to its initial state. The parameters are untouched and the next
//! `plhs` run starts at iteration 1 again.
//!
//! `sampling polish` refines the pool's unevaluated designs in place:
//! the deterministic swap descent of the combined objective (pooled pair
//! r², then space-filling energy under the τ guards) applied to the pool
//! as it stands — no seed, no scope, any pool. Evaluated designs (with a
//! results entry) are fixed context; every swap preserves each
//! parameter's value multiset, so the marginals and distributions are
//! invariant. The pass refines on the definition-range unit cube, the
//! same normalization the stats report measures: the reported r² is the
//! pooled pair r² `doe-stats sampling` computes on the pool. A no-op
//! (fewer than two movable designs or no parameter pairs) succeeds
//! without touching the file.
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
    /// Remove designs from the pool: all without a results entry
    /// (default) or every design together with the results (--force)
    #[command(name = "reset")]
    Reset(ResetArgs),
    /// Refine the pool's unevaluated designs in place (deterministic
    /// swap descent under the combined objective; evaluated designs are
    /// fixed context)
    #[command(name = "polish")]
    Polish(PolishArgs),
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

#[derive(Args)]
struct ResetArgs {
    /// Input JSON file; also the output file.
    file: PathBuf,
    /// Remove every design together with the whole results array,
    /// returning the pool to its initial state (params only, empty
    /// `designs`, the next augmentation starts at iteration 1 again).
    #[arg(long)]
    force: bool,
    /// Skip writing the backup file.
    #[arg(long)]
    no_backup: bool,
    /// Print one line of machine-readable JSON instead of the human
    /// summary (for benchmarks and result collection).
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct PolishArgs {
    /// Input JSON file; also the output file.
    file: PathBuf,
    /// Skip writing the backup file.
    #[arg(long)]
    no_backup: bool,
    /// Print one line of machine-readable JSON instead of the human
    /// summary (for benchmarks and result collection).
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init(args) => run_init(&args),
        Command::Plhs(args) => run(&args),
        Command::Stats(args) => run_stats(&args),
        Command::Reset(args) => run_reset(&args),
        Command::Polish(args) => run_polish(&args),
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
            "ae_energy": aug.ae_energy,
            "elapsed_ms": elapsed_ms,
        });
        // the pass reports what it did; without it there is nothing to
        // report (skipped or disabled)
        if let Some(p) = &aug.polish {
            report["polish_moves"] = serde_json::json!(p.moves);
            report["polish_score_before"] = serde_json::json!(p.score_before);
            report["polish_score_after"] = serde_json::json!(p.score_after);
            report["polish_ae_moves"] = serde_json::json!(p.ae_moves);
            report["polish_ae_before"] = serde_json::json!(p.ae_before);
            report["polish_ae_after"] = serde_json::json!(p.ae_after);
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
        if p.moves + p.ae_moves > 0 {
            summary.push_str(&format!(
                ", polished with {} swap(s) (r2 was {:.6}, AE was {:.3})",
                p.moves + p.ae_moves,
                p.score_before,
                p.ae_before
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

/// Removes designs from the pool: every design without a results entry,
/// or — with `--force` — every design together with the whole `results`
/// array. The parameters are untouched; the pool is written back with
/// the usual `.bak` backup. Removing nothing succeeds (idempotent).
fn run_reset(args: &ResetArgs) -> Result<String, CliError> {
    let (bytes, mut root) = doe_pool::read(&args.file).map_err(pool)?;
    // validate the pool first so structurally invalid files fail before
    // anything is removed
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let _designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let report = doe_pool::format::reset_designs(&mut root, args.force).map_err(validation)?;

    if !args.no_backup {
        doe_pool::backup(&args.file, &bytes).map_err(pool)?;
    }
    let out_bytes = doe_pool::serialize(&root).map_err(pool)?;
    doe_pool::write_atomic(&args.file, &out_bytes).map_err(pool)?;

    if args.json {
        return Ok(serde_json::json!({
            "subcommand": "reset",
            "mode": if args.force { "all" } else { "unevaluated" },
            "file": args.file.display().to_string(),
            "designs_before": report.designs_before,
            "designs_after": report.designs_after,
            "results_before": report.results_before,
            "results_after": report.results_after,
        })
        .to_string());
    }

    if args.force {
        return Ok(format!(
            "removed all {} designs and {} results",
            report.designs_before, report.results_before
        ));
    }
    Ok(format!(
        "removed {} designs ({} kept, {} results)",
        report.designs_before - report.designs_after,
        report.designs_after,
        report.results_before
    ))
}

/// Refines the pool's unevaluated designs in place: every design
/// without a results entry (the same name-based predicate as
/// `simio test` and `reset`) is movable, the evaluated designs are
/// fixed context bound to their simulation outputs. Deterministic —
/// no seed. The pool file is rewritten (with the `.bak` backup) only
/// when the pass applied at least one swap; a no-op leaves the file
/// untouched.
fn run_polish(args: &PolishArgs) -> Result<String, CliError> {
    let (bytes, mut root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let mut designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let results = doe_pool::format::parse_results(&root).map_err(validation)?;
    let solved: std::collections::BTreeSet<&str> =
        results.iter().map(|r| r.name.as_str()).collect();
    let movable: Vec<bool> = designs
        .iter()
        .map(|d| !solved.contains(d.name.as_str()))
        .collect();
    let evaluated_skipped = movable.iter().filter(|&&m| !m).count();
    let movable_count = designs.len() - evaluated_skipped;

    let started = std::time::Instant::now();
    let report = plhs::polish_pool(&params, &mut designs, &movable).map_err(validation)?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let moves = report.moves + report.ae_moves;

    if moves > 0 {
        if !args.no_backup {
            doe_pool::backup(&args.file, &bytes).map_err(pool)?;
        }
        let refined: Vec<plhs::Design> = designs
            .iter()
            .zip(&movable)
            .filter(|&(_, &is_movable)| is_movable)
            .map(|(design, _)| design.clone())
            .collect();
        doe_pool::format::update_design_values(&mut root, &params, &refined).map_err(validation)?;
        let out_bytes = doe_pool::serialize(&root).map_err(pool)?;
        doe_pool::write_atomic(&args.file, &out_bytes).map_err(pool)?;
    }

    if args.json {
        return Ok(serde_json::json!({
            "subcommand": "polish",
            "file": args.file.display().to_string(),
            "designs": designs.len(),
            "movable": movable_count,
            "evaluated_skipped": evaluated_skipped,
            "moves": moves,
            "r2_before": report.score_before,
            "r2_after": report.score_after,
            "ae_before": report.ae_before,
            "ae_after": report.ae_after,
            "elapsed_ms": elapsed_ms,
        })
        .to_string());
    }

    Ok(format!(
        "polished {movable_count} designs, r2 {:.6} -> {:.6}, AE {:.3} -> {:.3} ({moves} moves)",
        report.score_before, report.score_after, report.ae_before, report.ae_after
    ))
}
