//! The `sampling` command-line tool: Design of Experiments sampling methods.
//!
//! One subcommand per sampling method. `sampling plhs` is pooled Latin
//! Hypercube Sampling: it reads a JSON design file and adds new designs in
//! one of two modes — `--add N` refines inside the per-parameter scopes,
//! `--explore N` covers the whole surface (uniform over each parameter's
//! full definition range). The result is written back (same file by
//! default, with a `.bak` backup of the input unless disabled).
//!
//! `sampling stats` analyzes the evaluated results of the pool instead:
//! coverage, descriptive statistics, per-parameter correlations, scope
//! split and decile means. It is read-only judgment support before
//! expensive simulations are run.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod json;

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

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
    /// Pooled Latin Hypercube Sampling: augment a design pool with
    /// low-correlation designs, scoped (--add) or whole-surface (--explore)
    #[command(name = "plhs")]
    Plhs(PlhsArgs),
    /// Statistical analysis of the evaluated results: coverage, descriptive
    /// statistics, per-parameter correlations, scope split, decile means
    /// (read-only)
    #[command(name = "stats")]
    Stats(StatsArgs),
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
}

#[derive(Args)]
struct StatsArgs {
    /// Input JSON file with the pool and evaluated results.
    file: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
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
    let bytes = fs::read(&args.file).map_err(CliError::Io)?;
    let mut root: Value = serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Validation(format!("invalid JSON: {e}")))?;
    let params = json::parse_params(&root).map_err(validation)?;
    let designs = json::parse_designs(&root, &params).map_err(validation)?;
    let aug = if explore {
        plhs::explore(&params, &designs, m, args.seed, args.cap).map_err(validation)?
    } else {
        plhs::augment(&params, &designs, m, args.seed, args.cap).map_err(validation)?
    };

    if !args.no_backup {
        let mut backup = args.file.clone().into_os_string();
        backup.push(".bak");
        fs::write(Path::new(&backup), &bytes).map_err(CliError::Io)?;
    }

    json::append_new_designs(&mut root, &params, &aug.new_designs).map_err(validation)?;
    let out_bytes = serde_json::to_vec_pretty(&root)
        .map_err(|e| CliError::Validation(format!("serialization failed: {e}")))?;
    let out = args.output.clone().unwrap_or_else(|| args.file.clone());
    write_atomic(&out, &out_bytes)?;

    Ok(format!(
        "{verb} {} designs (iteration {}), score {:.6}",
        aug.new_designs.len(),
        aug.iteration,
        aug.score
    ))
}

/// Analyzes the evaluated results of the pool and formats the statistics
/// report. Read-only: no files are written.
fn run_stats(args: &StatsArgs) -> Result<String, CliError> {
    let bytes = fs::read(&args.file).map_err(CliError::Io)?;
    let root: Value = serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Validation(format!("invalid JSON: {e}")))?;
    let params = json::parse_params(&root).map_err(validation)?;
    let designs = json::parse_designs(&root, &params).map_err(validation)?;
    let results = json::parse_results(&root).map_err(validation)?;
    let stats = doe_stats::analyze(&params, &designs, &results).map_err(validation)?;
    Ok(format_stats_report(&stats, designs.len(), results.len()))
}

/// Formats the statistics as a human-readable report (one block per result
/// variable; statistics only, the judgment stays with the reader).
fn format_stats_report(
    stats: &[doe_stats::VariableStats],
    total: usize,
    evaluated: usize,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("pool: {total} designs, {evaluated} evaluated\n"));
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

/// Writes via a temporary file and an atomic rename, so the output file is
/// either the old or the new content, never a partial write.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).map_err(CliError::Io)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(CliError::Io(e))
        }
    }
}
