//! Errors of the pool file operations.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced while reading, parsing or writing a pool file.
#[derive(Debug, Error)]
pub enum Error {
    /// Reading or writing the file failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The pool content is invalid.
    #[error(transparent)]
    Validation(#[from] doe_types::Error),
}
