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
//!   canonical pretty JSON on disk without ever leaving a partial write.

pub mod error;
pub mod format;

use std::path::Path;

use serde_json::Value;

pub use error::{Error, Result};

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
