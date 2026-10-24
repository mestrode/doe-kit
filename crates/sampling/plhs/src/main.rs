//! The `plhs` command-line tool: pooled Latin Hypercube Sampling.
//!
//! Reads a JSON design file, adds `--add N` new designs scoped to the
//! per-parameter scopes, and writes the result back (same file by default,
//! with a `.bak` backup of the input unless disabled).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod json;

use clap::Parser;
use serde_json::Value;

#[derive(Parser)]
#[command(
    name = "plhs",
    version,
    about = "Pooled Latin Hypercube Sampling: augment a design pool with low-correlation designs scoped to a parameter sub-range"
)]
struct Args {
    /// Input JSON file; also the output file unless --output is given.
    file: PathBuf,
    /// Number of new designs to add.
    #[arg(short, long)]
    add: usize,
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

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
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

fn run(args: &Args) -> Result<String, CliError> {
    if args.add == 0 {
        return Err(CliError::Validation("--add must be at least 1".to_owned()));
    }
    let bytes = fs::read(&args.file).map_err(CliError::Io)?;
    let mut root: Value = serde_json::from_slice(&bytes)
        .map_err(|e| CliError::Validation(format!("invalid JSON: {e}")))?;
    let params = json::parse_params(&root).map_err(validation)?;
    let designs = json::parse_designs(&root, &params).map_err(validation)?;
    let aug =
        plhs::augment(&params, &designs, args.add, args.seed, args.cap).map_err(validation)?;

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
        "added {} designs (iteration {}), score {:.6}",
        aug.new_designs.len(),
        aug.iteration,
        aug.score
    ))
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
