//! Error types for [`plhs`](crate).

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced while validating input or sampling.
#[derive(Debug, Error)]
pub enum Error {
    /// A parameter definition is invalid.
    #[error("invalid parameter '{name}': {reason}")]
    Parameter { name: String, reason: String },

    /// A scope entry is invalid or names an unknown parameter.
    #[error("invalid scope entry '{name}': {reason}")]
    ScopeEntry { name: String, reason: String },

    /// The scope is missing an entry for a declared parameter.
    #[error("scope is missing an entry for parameter '{0}'")]
    MissingScopeEntry(String),

    /// A design is invalid.
    #[error("invalid design '{name}': {reason}")]
    Design { name: String, reason: String },

    /// A parameter pair cannot be scored.
    #[error("parameters '{a}' and '{b}': {reason}")]
    Pair {
        a: String,
        b: String,
        reason: String,
    },

    /// Generic input validation failure.
    #[error("{0}")]
    Invalid(String),

    /// An internal invariant was violated; this indicates a bug in `plhs`.
    #[error("internal invariant violated: {0}")]
    Invariant(String),
}
