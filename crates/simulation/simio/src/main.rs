//! The `simio` command-line tool: simulation input/output for the
//! doe-kit design pool.
//!
//! Simulation results enter the pool as `results` entries — one object
//! per evaluated design, its `name` matching the design, every numeric
//! field a result variable. Planned subcommands move designs out to
//! external simulators (`export`, `exec`) and their results back
//! (`import`); until they land, `test` is the in-process stand-in:
//!
//! `simio test <file>` evaluates the deterministic analytic example
//! functions (`linear`, `bowl`, `wave`) on every design of the pool
//! that has no results entry yet (solved designs stay untouched),
//! appends the new entries to `results` and writes the pool back
//! (`.bak` backup of the input unless `--no-backup`). The parameters
//! `a`, `b`, `c` are resolved by name; extra parameters are allowed
//! and ignored.
//!
//! `--json` prints one line of machine-readable JSON instead of the
//! human summary; errors stay human on stderr with the usual exit
//! codes (0 success, 1 validation, 2 I/O).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "simio",
    version,
    about = "Simulation input/output for the doe-kit design pool; each operation is a subcommand"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Evaluate the analytic example functions (linear, bowl, wave) on
    /// every unevaluated design and write the results into the pool
    #[command(name = "test")]
    Test(TestArgs),
}

#[derive(Args)]
struct TestArgs {
    /// Input JSON pool; the results are written into this file.
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
        Command::Test(args) => run_test(&args),
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

fn validation(e: doe_types::Error) -> CliError {
    CliError::Validation(e.to_string())
}

fn pool(e: doe_pool::Error) -> CliError {
    match e {
        doe_pool::Error::Validation(e) => CliError::Validation(e.to_string()),
        doe_pool::Error::Io(e) => CliError::Io(e),
    }
}

/// Evaluates the analytic example functions on every unevaluated design
/// and writes the results into the pool. Already-solved designs are
/// skipped and left untouched; when there is nothing to evaluate, the
/// file is not written at all.
fn run_test(args: &TestArgs) -> Result<String, CliError> {
    let (bytes, mut root) = doe_pool::read(&args.file).map_err(pool)?;
    let params = doe_pool::format::parse_params(&root).map_err(validation)?;
    let designs = doe_pool::format::parse_designs(&root, &params).map_err(validation)?;
    let existing = doe_pool::format::parse_results(&root).map_err(validation)?;
    let solved: BTreeSet<String> = existing.iter().map(|r| r.name.clone()).collect();
    let skipped = designs.iter().filter(|d| solved.contains(&d.name)).count();
    let new = doe_simio::analytic::evaluate(&params, &designs, &solved).map_err(validation)?;

    if !new.is_empty() {
        if !args.no_backup {
            doe_pool::backup(&args.file, &bytes).map_err(pool)?;
        }
        doe_pool::format::append_results(&mut root, &params, &new).map_err(validation)?;
        let out_bytes = doe_pool::serialize(&root).map_err(pool)?;
        doe_pool::write_atomic(&args.file, &out_bytes).map_err(pool)?;
    }

    if args.json {
        return Ok(serde_json::json!({
            "subcommand": "test",
            "file": args.file.display().to_string(),
            "designs": designs.len(),
            "evaluated": new.len(),
            "skipped": skipped,
            "variables": doe_simio::analytic::VARIABLES,
        })
        .to_string());
    }
    let mut summary = format!("evaluated {} designs", new.len());
    if skipped > 0 {
        summary.push_str(&format!(" ({skipped} already solved, skipped)"));
    }
    summary.push_str(&format!(
        ", results: {}",
        doe_simio::analytic::VARIABLES.join(", ")
    ));
    Ok(summary)
}
