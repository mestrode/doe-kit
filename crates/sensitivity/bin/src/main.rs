//! The `doe-stats` command-line tool: statistical views of the pool.
//!
//! Three subcommands, one per view, plus the bare invocation
//! `doe-stats <file>` which prints the full report (all three
//! sections in order):
//!
//! - `doe-stats input <file>`: the parameter definitions —
//!   distribution, definition range, the Gaussian's mean and sigma,
//!   and the scope with its share of the definition range. Needs no
//!   designs and no results.
//! - `doe-stats sampling <file>`: the statistics of the sampling
//!   points — space-filling metrics in the unit cube (min/mean/median
//!   pairwise distance, mean nearest-neighbor distance, centered L2
//!   discrepancy, the pooled pair r2 that plhs optimizes) plus the
//!   occupancy and pair summaries. Needs at least one design.
//! - `doe-stats output <file>`: the statistics of the evaluated
//!   results — per-variable coverage and descriptive statistics,
//!   per-parameter correlations, scope split and decile means, plus
//!   the pair correlations between the result variables. Needs
//!   results.
//!
//! The full report runs the sections in order and stops at the first
//! failing one. Statistics only, read-only: no files are written and
//! the judgment stays with the reader.
//!
//! Result entries are validated only where results are read: `input`
//! and `sampling` ignore a result name that matches no design; `output`
//! and the full report reject it.
//!
//! Every view accepts `--json` (one line of machine-readable JSON on
//! stdout instead of the human report) and `--verbose` (the full
//! detail tier instead of the compact one — no effect on the input
//! section, which is always printed in full). Errors stay human on
//! stderr with the usual exit codes: 0 success, 1 validation error,
//! 2 I/O error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "doe-stats",
    version,
    about = "Statistical views of a doe pool: input definitions, sampling points, outputs"
)]
struct Cli {
    /// Input JSON pool file; without a subcommand the full report is
    /// printed (input, sampling and output sections)
    file: Option<PathBuf>,
    /// Print one line of machine-readable JSON instead of the human
    /// report (for benchmarks and result collection).
    #[arg(long)]
    json: bool,
    /// Switch to the full detail tier; no effect on the input section
    /// (definitions are always printed in full).
    #[arg(long)]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Parameter definitions: distribution, definition range and scope
    /// (works without designs and without results)
    #[command(name = "input")]
    Input(ViewArgs),
    /// Statistics of the sampling points: space-filling metrics,
    /// occupancy and pair summaries (needs at least one design)
    #[command(name = "sampling")]
    Sampling(ViewArgs),
    /// Statistics of the evaluated results: per-variable coverage and
    /// correlations plus the pairs between the result variables (needs
    /// results)
    #[command(name = "output")]
    Output(ViewArgs),
}

#[derive(Args)]
struct ViewArgs {
    /// Input JSON pool file.
    file: PathBuf,
    /// Print one line of machine-readable JSON instead of the human
    /// report (for benchmarks and result collection).
    #[arg(long)]
    json: bool,
    /// Switch to the full detail tier; no effect on the input section.
    #[arg(long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Input(args)) => run_input(&args),
        Some(Command::Sampling(args)) => run_sampling(&args),
        Some(Command::Output(args)) => run_output(&args),
        None => match cli.file {
            Some(file) => run_all(&file, cli.json, cli.verbose),
            None => {
                eprintln!("error: a pool file is required (see 'doe-stats --help' for usage)");
                return ExitCode::from(2);
            }
        },
    };
    match result {
        Ok(report) => {
            println!("{report}");
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

fn validation(e: doe_types::Error) -> CliError {
    CliError::Validation(e.to_string())
}

fn pool(e: doe_pool::Error) -> CliError {
    match e {
        doe_pool::Error::Validation(e) => CliError::Validation(e.to_string()),
        doe_pool::Error::Io(e) => CliError::Io(e),
    }
}

fn json(e: serde_json::Error) -> CliError {
    CliError::Validation(e.to_string())
}

fn file_str(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// Input view: the parameter definitions only. Needs neither designs
/// nor results; result entries are not read and not validated.
fn run_input(args: &ViewArgs) -> Result<String, CliError> {
    let (_, root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let input = doe_stats::analyze_inputs(&params);
    if args.json {
        return doe_stats_bin::json_input("input", &file_str(&args.file), &input).map_err(json);
    }
    Ok(doe_stats_bin::human_input(&input).trim_end().to_owned())
}

/// Sampling view: the statistics of the sampling points. The results
/// are parsed for the evaluated count only — result names are not
/// validated here, this view never reads the values.
fn run_sampling(args: &ViewArgs) -> Result<String, CliError> {
    let (_, root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let results = doe_pool::format::parse_results(&root).map_err(validation)?;
    let pool = doe_stats::analyze_pool(&params, &designs).map_err(validation)?;
    if args.json {
        return doe_stats_bin::json_sampling(
            "sampling",
            &file_str(&args.file),
            results.len(),
            &pool,
            args.verbose,
        )
        .map_err(json);
    }
    Ok(
        doe_stats_bin::human_sampling(&pool, results.len(), args.verbose)
            .trim_end()
            .to_owned(),
    )
}

/// Output view: the statistics of the evaluated results plus the pair
/// correlations between the result variables. Result names are
/// validated here — this is the view that reads them.
fn run_output(args: &ViewArgs) -> Result<String, CliError> {
    let (_, root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let results = doe_pool::format::parse_results(&root).map_err(validation)?;
    let (stats, pairs) = analyze_results(&params, &designs, &results)?;
    if args.json {
        return doe_stats_bin::json_output(
            "output",
            &file_str(&args.file),
            results.len(),
            &stats,
            &pairs,
            args.verbose,
        )
        .map_err(json);
    }
    Ok(doe_stats_bin::human_output(&stats, &pairs, args.verbose)
        .trim_end()
        .to_owned())
}

/// Full report: the sections run in order (input, sampling, output)
/// and the first failing one ends the run — a fresh pool fails at the
/// sampling section, a design-less-but-evaluated pool at the output
/// section.
fn run_all(file: &Path, json_flag: bool, verbose: bool) -> Result<String, CliError> {
    let (_, root) = doe_pool::read(file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let results = doe_pool::format::parse_results(&root).map_err(validation)?;

    // input section (definitions cannot fail), then the sampling
    // section (needs designs), then the output section (needs results)
    let input = doe_stats::analyze_inputs(&params);
    let pool = doe_stats::analyze_pool(&params, &designs).map_err(validation)?;
    let (stats, pairs) = analyze_results(&params, &designs, &results)?;

    let data = doe_stats_bin::ReportData {
        input: &input,
        pool: &pool,
        results: &stats,
        pairs: &pairs,
        evaluated: results.len(),
    };

    if json_flag {
        return doe_stats_bin::json_report("all", &file_str(file), &data, verbose).map_err(json);
    }
    Ok(doe_stats_bin::human_report(&data, verbose))
}

/// Runs the output analyses: empty results fail, result names are
/// validated (against the designs), then the per-variable statistics
/// and the output pairs are computed.
fn analyze_results(
    params: &[doe_types::Parameter],
    designs: &[doe_types::Design],
    results: &[doe_types::Results],
) -> Result<(Vec<doe_stats::VariableStats>, doe_stats::ResultPairStats), CliError> {
    if results.is_empty() {
        return Err(CliError::Validation("no results to analyze".to_owned()));
    }
    let stats = doe_stats::analyze(params, designs, results).map_err(validation)?;
    let pairs = doe_stats::analyze_result_pairs(designs, results).map_err(validation)?;
    Ok((stats, pairs))
}
