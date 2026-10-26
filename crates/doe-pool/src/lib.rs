//! The doe-kit pool file format.
//!
//! Every crate and binary in the workspace that touches a design pool file
//! goes through this crate, so that reading, parsing, backup and writing
//! behave identically everywhere:
//!
//! - [`read`] reads and parses a pool file into a `serde_json::Value`
//!   (unknown fields and key order are preserved verbatim),
//! - [`format`] extracts the typed model (`params`, `designs`, `results`)
//!   from the value and appends new designs back into it,
//! - [`backup`] writes the `<path>.bak` backup of the original bytes,
//! - [`serialize`] and [`write_atomic`] turn the value back into the
//!   canonical pretty JSON on disk without ever leaving a partial write,
//! - [`skeleton`] builds the example pool that starts a new file.

pub mod error;
pub mod format;

use std::path::Path;

use serde_json::Value;

pub use error::{Error, Result};

/// Returns the example pool written by `sampling init`: exactly three
/// example parameters — `a` and `b` uniform over `[0, 10]`, `c` gaussian
/// over `[0, 10]` with mean 5 and sigma 2 (the inputs of the analytic
/// example simulators) — an empty `designs` list, no `scope` (defaults
/// to the full parameter ranges) and no `results` (none evaluated yet).
/// The output is immediately parseable so the other subcommands run on
/// it once the user edits `params`.
pub fn skeleton() -> Value {
    serde_json::json!({
        "params": [
            {
                "name": "a",
                "min": 0,
                "max": 10,
                "distribution": "uniform"
            },
            {
                "name": "b",
                "min": 0,
                "max": 10,
                "distribution": "uniform"
            },
            {
                "name": "c",
                "min": 0,
                "max": 10,
                "mean": 5,
                "sigma": 2,
                "distribution": "gaussian"
            }
        ],
        "designs": []
    })
}

/// Reads the file at `path` and parses it as JSON.
///
/// Returns the raw bytes (for the backup) alongside the parsed value, so
/// the original content can be restored without reading the file twice.
pub fn read(path: &Path) -> Result<(Vec<u8>, Value)> {
    let bytes = std::fs::read(path)?;
    let root: Value = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Validation(doe_types::Error::Invalid(format!("invalid JSON: {e}"))))?;
    Ok((bytes, root))
}

/// Writes the backup of the original pool bytes to `<path>.bak`.
pub fn backup(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".bak");
    std::fs::write(std::path::Path::new(&backup), bytes)?;
    Ok(())
}

/// Serializes the pool value into the canonical pretty JSON.
pub fn serialize(root: &Value) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(root).map_err(|e| {
        Error::Validation(doe_types::Error::Invalid(format!(
            "serialization failed: {e}"
        )))
    })
}

/// Writes via a temporary file and an atomic rename, so the output file is
/// either the old or the new content, never a partial write.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(Error::Io(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_parses_into_three_example_params_and_no_designs() {
        let root = skeleton();
        let params = format::parse_params(&root).expect("skeleton params must parse");
        assert_eq!(params.len(), 3, "exactly three example parameters");
        assert_eq!(params[0].name, "a");
        assert_eq!(params[0].min, 0.0);
        assert_eq!(params[0].max, 10.0);
        assert_eq!(params[0].distribution, doe_types::Distribution::Uniform);
        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].min, 0.0);
        assert_eq!(params[1].max, 10.0);
        assert_eq!(params[1].distribution, doe_types::Distribution::Uniform);
        assert_eq!(params[2].name, "c");
        assert_eq!(params[2].min, 0.0);
        assert_eq!(params[2].max, 10.0);
        assert_eq!(
            params[2].distribution,
            doe_types::Distribution::Gaussian {
                mean: 5.0,
                sigma: 2.0
            }
        );
        assert!(root.get("scope").is_none(), "no scope: full range");
        assert!(root.get("results").is_none(), "no results until evaluated");
        assert!(root["designs"].as_array().unwrap().is_empty());
        let designs = format::parse_designs(&root, &params).expect("empty pool must parse");
        assert!(designs.is_empty());
    }
}
