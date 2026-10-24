//! Shared data types for the doe-kit workspace.
//!
//! Cross-cutting types used by every crate in the workspace:
//!
//! - [`error`]: the workspace-wide validation `Error` and `Result` alias,
//! - [`scaling`]: parameter definitions, distributions and scope transforms,
//! - [`Design`]: one experiment design of the pool,
//! - [`Results`]: one evaluated design's numeric result variables.

use std::collections::BTreeMap;

pub mod error;
pub mod scaling;

pub use error::{Error, Result};
pub use scaling::{Distribution, Parameter};

/// One design of the experiment: a name and one value per parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    /// Design name.
    pub name: String,
    /// Values aligned with the parameter slice given to the sampler.
    pub values: Vec<f64>,
}

/// One evaluated design: the numeric fields of its result entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// Design name; must match a design in the pool.
    pub name: String,
    /// Numeric result variables (e.g. `energy: 42.1`). Non-numeric fields
    /// are the caller's business and never reach this type.
    pub values: BTreeMap<String, f64>,
}
