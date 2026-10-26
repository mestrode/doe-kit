//! Deterministic analytic example functions for testing.
//!
//! The functions are defined over the three example parameters of the
//! `sampling init` skeleton — `a` uniform over `[0, 10]`, `b` uniform
//! over `[-1, 1]`, `c` gaussian over `[0, 5]` — resolved by name, so
//! pools from other sources work too as long as they declare `a`, `b`
//! and `c`. Extra parameters are allowed and ignored: a simulator only
//! reads its inputs.
//!
//! The three result variables are graded by difficulty, so they double
//! as a testbed for the planned metamodel and sensitivity crates (each
//! has a known closed form, and surrogates can be checked against the
//! ground truth):
//!
//! - `linear`: pure first-order baseline — any metamodel that fails
//!   this is broken;
//! - `bowl`: convex quadratic with a mild interaction — zero at the
//!   pool's center point, the interaction shifts the true minimum
//!   slightly below it;
//! - `wave`: damped oscillation plus a second oscillation source —
//!   non-polynomial, non-monotonic, the hard case.

use std::collections::{BTreeMap, BTreeSet};

use doe_types::{Design, Error, Parameter, Result, Results};

/// The result variables written per design, in report order.
pub const VARIABLES: [&str; 3] = ["linear", "bowl", "wave"];

/// `0.7854·a + 2.5·b + 0.5·c` — pure first-order baseline.
// 0.7854 is the deliberate constant of the settled formula, not an
// approximation of pi/4.
#[allow(clippy::approx_constant)]
pub fn linear(a: f64, b: f64, c: f64) -> f64 {
    0.7854 * a + 2.5 * b + 0.5 * c
}

/// `(a−5)² + b² + (c−2.5)² + 0.3·a·b` — convex quadratic bowl over
/// the three parameters, zero at the pool's center point `(5, 0, 2.5)`;
/// the mild interaction term shifts the true minimum slightly below
/// that point into `b < 0`.
pub fn bowl(a: f64, b: f64, c: f64) -> f64 {
    (a - 5.0) * (a - 5.0) + b * b + (c - 2.5) * (c - 2.5) + 0.3 * a * b
}

/// `10·sin(0.6283·a)·exp(−(b+1)/2) + 0.5·sin(1.2566·c)` — damped
/// oscillation (one full sine period over `a`'s range, damping factor
/// 2.7 over `b`'s range) plus a second oscillation source in `c`.
pub fn wave(a: f64, b: f64, c: f64) -> f64 {
    10.0 * (0.6283 * a).sin() * (-((b + 1.0) / 2.0)).exp() + 0.5 * (1.2566 * c).sin()
}

/// Resolves the value indices of `a`, `b`, `c` in the parameter list.
fn indices(params: &[Parameter]) -> Result<(usize, usize, usize)> {
    let find = |name: &str| {
        params.iter().position(|p| p.name == name).ok_or_else(|| {
            Error::Invalid(format!(
                "missing parameter '{name}' (test needs parameters a, b, c)"
            ))
        })
    };
    Ok((find("a")?, find("b")?, find("c")?))
}

/// Evaluates every design whose name is not in `solved` (a design with
/// a results entry is already solved and stays untouched); extra
/// parameters are ignored. Returns the new [`Results`] in design
/// order.
pub fn evaluate(
    params: &[Parameter],
    designs: &[Design],
    solved: &BTreeSet<String>,
) -> Result<Vec<Results>> {
    let (ia, ib, ic) = indices(params)?;
    let mut out = Vec::new();
    for design in designs {
        if solved.contains(&design.name) {
            continue;
        }
        let (a, b, c) = (design.values[ia], design.values[ib], design.values[ic]);
        let mut values = BTreeMap::new();
        values.insert("linear".to_owned(), linear(a, b, c));
        values.insert("bowl".to_owned(), bowl(a, b, c));
        values.insert("wave".to_owned(), wave(a, b, c));
        out.push(Results {
            name: design.name.clone(),
            values,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    // the formula constants (0.7854, 1.5708) are deliberate, not
    // approximations of pi/4 resp. pi/2
    #![allow(clippy::approx_constant)]
    use super::*;

    #[test]
    fn linear_is_the_weighted_sum() {
        assert!((linear(2.0, 1.0, 1.0) - (1.5708 + 2.5 + 0.5)).abs() < 1e-12);
        assert_eq!(linear(0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn bowl_has_its_minimum_at_the_pool_center() {
        assert_eq!(bowl(5.0, 0.0, 2.5), 0.0);
        assert!((bowl(10.0, 1.0, 5.0) - (25.0 + 1.0 + 6.25 + 3.0)).abs() < 1e-12);
    }

    #[test]
    fn wave_damps_and_oscillates() {
        // at a=2.5 the sine peaks (0.6283·2.5 ≈ π/2); c=0 kills the
        // second term, so b=-1 (no damping) gives amplitude 10
        let peak = wave(2.5, -1.0, 0.0);
        assert!((peak - 10.0).abs() < 1e-6, "peak: {peak}");
        // full damping at b=1 is one e-fold
        let damped = wave(2.5, 1.0, 0.0);
        assert!(
            (damped - peak / std::f64::consts::E).abs() < 1e-6,
            "damped: {damped}"
        );
        // c contributes one full oscillation over [0, 5]
        assert!((wave(0.0, -1.0, 2.5) - 0.0).abs() < 1e-3, "c half period");
    }

    #[test]
    fn variables_are_in_report_order() {
        assert_eq!(VARIABLES, ["linear", "bowl", "wave"]);
    }

    fn params(names: &[&str]) -> Vec<Parameter> {
        names
            .iter()
            .map(|n| Parameter::uniform(n.to_owned(), 0.0, 10.0).unwrap())
            .collect()
    }

    fn design(name: &str, values: &[f64]) -> Design {
        Design {
            name: name.to_owned(),
            values: values.to_vec(),
        }
    }

    #[test]
    fn evaluate_resolves_params_by_name_and_ignores_extras() {
        let params = params(&["d", "a", "b", "c"]);
        let designs = [design("1.1", &[4.0, 5.0, 0.0, 2.5])];
        let out = evaluate(&params, &designs, &BTreeSet::new()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "1.1");
        assert_eq!(out[0].values["bowl"], 0.0);
        assert!((out[0].values["linear"] - (0.7854 * 5.0 + 0.5 * 2.5)).abs() < 1e-12);
        // at the pool center both oscillations cross zero (a=5 is the
        // half period of its sine, c=2.5 the half period of the second)
        assert!(
            out[0].values["wave"].abs() < 1e-3,
            "wave: {}",
            out[0].values["wave"]
        );
    }

    #[test]
    fn evaluate_skips_solved_designs() {
        let params = params(&["a", "b", "c"]);
        let designs = [
            design("1.1", &[5.0, 0.0, 2.5]),
            design("1.2", &[0.0, 0.0, 0.0]),
        ];
        let solved = BTreeSet::from(["1.1".to_owned()]);
        let out = evaluate(&params, &designs, &solved).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "1.2");
    }

    #[test]
    fn evaluate_names_the_missing_parameter() {
        let params = params(&["a", "c"]);
        let designs = [design("1.1", &[5.0, 2.5])];
        let err = evaluate(&params, &designs, &BTreeSet::new()).unwrap_err();
        assert!(err.to_string().contains("missing parameter 'b'"), "{err}");
    }
}
