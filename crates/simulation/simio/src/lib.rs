//! The doe-kit simulation input/output crate.
//!
//! Simulation results enter the pool as `results` entries — one object
//! per evaluated design, its `name` matching the design, every numeric
//! field a result variable. This crate is the home of the tooling that
//! moves designs out to simulators and results back into the pool:
//!
//! - [`analytic`]: the deterministic example functions (`linear`,
//!   `bowl`, `wave`) that produce known results for testing, so the
//!   whole workflow — and later the metamodel crates — can be
//!   exercised and validated without an external simulator.

pub mod analytic;
