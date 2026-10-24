//! Parameter definitions and scope transforms.
//!
//! Every parameter has a definition range `[min, max]` and a scope
//! `[scope_min, scope_max]` (which defaults to the definition range). Values
//! are mapped into `[0, 1]` through the distribution's CDF truncated to the
//! definition range, then affinely remapped from the scope:
//!
//! - `transform(x)` maps a scope value to `t` in `[0, 1]` (probability space),
//! - `inverse_transform(t)` maps back to a real value.
//!
//! All sampling and scoring operates on transformed values; real values are
//! produced only for output.

use std::f64::consts::SQRT_2;

use crate::error::{Error, Result};

/// Probability distribution of a parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Distribution {
    /// Uniform over the definition range (default).
    Uniform,
    /// Gaussian truncated to the definition range.
    Gaussian {
        /// Mean of the untruncated Gaussian.
        mean: f64,
        /// Standard deviation of the untruncated Gaussian (must be positive).
        sigma: f64,
    },
}

impl Distribution {
    /// Lower-case name as used in the JSON format.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Gaussian { .. } => "gaussian",
        }
    }
}

/// A parameter definition with its resolved scope.
#[derive(Debug, Clone)]
pub struct Parameter {
    /// Parameter name.
    pub name: String,
    /// Definition range lower bound.
    pub min: f64,
    /// Definition range upper bound.
    pub max: f64,
    /// Probability distribution (truncated to `[min, max]`).
    pub distribution: Distribution,
    /// Scope lower bound (within `[min, max]`).
    pub scope_min: f64,
    /// Scope upper bound (within `[min, max]`).
    pub scope_max: f64,
}

impl Parameter {
    /// Creates a uniform parameter over `[min, max]`; the scope defaults to
    /// the definition range.
    pub fn uniform(name: impl Into<String>, min: f64, max: f64) -> Result<Self> {
        Self::build(name.into(), min, max, Distribution::Uniform)
    }

    /// Creates a Gaussian parameter truncated to `[min, max]`; the scope
    /// defaults to the definition range.
    pub fn gaussian(
        name: impl Into<String>,
        min: f64,
        max: f64,
        mean: f64,
        sigma: f64,
    ) -> Result<Self> {
        Self::build(
            name.into(),
            min,
            max,
            Distribution::Gaussian { mean, sigma },
        )
    }

    fn build(name: String, min: f64, max: f64, distribution: Distribution) -> Result<Self> {
        if !min.is_finite() || !max.is_finite() {
            return Err(Self::err(&name, "'min' and 'max' must be finite"));
        }
        if min >= max {
            return Err(Self::err(&name, "'min' must be less than 'max'"));
        }
        if let Distribution::Gaussian { mean, sigma } = distribution {
            if !mean.is_finite() {
                return Err(Self::err(&name, "'mean' must be finite"));
            }
            if !sigma.is_finite() || sigma <= 0.0 {
                return Err(Self::err(&name, "'sigma' must be positive"));
            }
        }
        Ok(Self {
            name,
            min,
            max,
            distribution,
            scope_min: min,
            scope_max: max,
        })
    }

    fn err(name: &str, reason: &str) -> Error {
        Error::Parameter {
            name: name.to_owned(),
            reason: reason.to_owned(),
        }
    }

    /// Restricts the scope to `[min, max]`, a sub-range of the definition
    /// range. Consumes and returns the parameter.
    pub fn with_scope(mut self, min: f64, max: f64) -> Result<Self> {
        if !min.is_finite() || !max.is_finite() {
            return Err(Error::ScopeEntry {
                name: self.name.clone(),
                reason: "'min' and 'max' must be finite".to_owned(),
            });
        }
        if min >= max {
            return Err(Error::ScopeEntry {
                name: self.name.clone(),
                reason: "'min' must be less than 'max'".to_owned(),
            });
        }
        if min < self.min || max > self.max {
            return Err(Error::ScopeEntry {
                name: self.name.clone(),
                reason: "the scope must lie within the parameter definition range".to_owned(),
            });
        }
        self.scope_min = min;
        self.scope_max = max;
        Ok(self)
    }

    /// Whether a value lies inside the scope (inclusive bounds).
    pub fn in_scope(&self, x: f64) -> bool {
        x >= self.scope_min && x <= self.scope_max
    }

    /// Returns a copy of this parameter as it is treated by explore mode
    /// (`plhs::explore`): uniform distribution over the full definition
    /// range `[min, max]`, ignoring any scope and any declared non-uniform
    /// distribution.
    pub fn full_surface(&self) -> Self {
        let mut full = self.clone();
        full.distribution = Distribution::Uniform;
        full.scope_min = full.min;
        full.scope_max = full.max;
        full
    }

    /// CDF of the distribution truncated to the definition range; maps
    /// `[min, max]` to `[0, 1]`.
    fn cdf_trunc(&self, x: f64) -> f64 {
        match self.distribution {
            Distribution::Uniform => (x - self.min) / (self.max - self.min),
            Distribution::Gaussian { mean, sigma } => {
                let lo = normal_cdf((self.min - mean) / sigma);
                let hi = normal_cdf((self.max - mean) / sigma);
                (normal_cdf((x - mean) / sigma) - lo) / (hi - lo)
            }
        }
    }

    /// Inverse of [`Parameter::cdf_trunc`].
    fn cdf_trunc_inv(&self, u: f64) -> f64 {
        match self.distribution {
            Distribution::Uniform => self.min + u * (self.max - self.min),
            Distribution::Gaussian { mean, sigma } => {
                let lo = normal_cdf((self.min - mean) / sigma);
                let hi = normal_cdf((self.max - mean) / sigma);
                mean + sigma * normal_cdf_inv(lo + u * (hi - lo))
            }
        }
    }

    /// Scope-transformed value `t` in `[0, 1]` (probability space of the
    /// distribution inside the scope).
    pub fn transform(&self, x: f64) -> f64 {
        let lo = self.cdf_trunc(self.scope_min);
        let hi = self.cdf_trunc(self.scope_max);
        (self.cdf_trunc(x) - lo) / (hi - lo)
    }

    /// Inverse of [`Parameter::transform`]: maps `t` in `[0, 1]` to a real
    /// value inside the scope.
    pub fn inverse_transform(&self, t: f64) -> f64 {
        let lo = self.cdf_trunc(self.scope_min);
        let hi = self.cdf_trunc(self.scope_max);
        self.cdf_trunc_inv(lo + t * (hi - lo))
    }
}

/// Standard normal CDF; absolute error below 1.5e-7.
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / SQRT_2)
}

/// Inverse standard normal CDF, computed by bisection on [`normal_cdf`].
/// Deterministic; accurate to roughly 1e-6 (limited by `normal_cdf`).
/// Requires `p` strictly inside `(0, 1)`.
pub fn normal_cdf_inv(p: f64) -> f64 {
    debug_assert!((0.0..=1.0).contains(&p));
    let (mut lo, mut hi) = (-40.0_f64, 40.0);
    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        if normal_cdf(mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Complementary error function, Abramowitz & Stegun 7.1.26
/// (absolute error < 1.5e-7).
fn erfc(x: f64) -> f64 {
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * ax);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf_pos = 1.0 - poly * (-ax * ax).exp();
    if x >= 0.0 {
        1.0 - erf_pos
    } else {
        1.0 + erf_pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param_uniform_scoped() -> Parameter {
        Parameter::uniform("x", 0.0, 10.0)
            .unwrap()
            .with_scope(2.0, 8.0)
            .unwrap()
    }

    fn param_gaussian_scoped() -> Parameter {
        Parameter::gaussian("y", 0.0, 10.0, 5.0, 2.0)
            .unwrap()
            .with_scope(3.0, 7.0)
            .unwrap()
    }

    #[test]
    fn uniform_transform_maps_scope_to_unit() {
        let p = param_uniform_scoped();
        assert!((p.transform(2.0)).abs() < 1e-12);
        assert!((p.transform(8.0) - 1.0).abs() < 1e-12);
        assert!((p.transform(5.0) - 0.5).abs() < 1e-12);
        assert!((p.transform(2.0 + 0.25 * (8.0 - 2.0)) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn gaussian_transform_maps_scope_to_unit() {
        let p = param_gaussian_scoped();
        assert!(p.transform(3.0).abs() < 1e-6);
        assert!((p.transform(7.0) - 1.0).abs() < 1e-6);
        // mean is the distribution center
        assert!((p.transform(5.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn transform_roundtrip() {
        for p in [param_uniform_scoped(), param_gaussian_scoped()] {
            for i in 0..=10 {
                let t = i as f64 / 10.0;
                let x = p.inverse_transform(t);
                let back = p.transform(x);
                assert!(
                    (back - t).abs() < 1e-5,
                    "{}: t={t} -> x={x} -> {back}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn transform_maps_definition_range_outside_scope() {
        let p = param_uniform_scoped();
        // a value below the scope transforms below 0, above the scope above 1
        assert!(p.transform(1.0) < 0.0);
        assert!(p.transform(9.0) > 1.0);
    }

    #[test]
    fn normal_cdf_known_values() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-9);
        assert!((normal_cdf(1.959_964) - 0.975).abs() < 1e-6);
        assert!((normal_cdf(-1.959_964) - 0.025).abs() < 1e-6);
    }

    #[test]
    fn normal_cdf_inv_known_values() {
        assert!((normal_cdf_inv(0.5)).abs() < 1e-5);
        assert!((normal_cdf_inv(0.975) - 1.959_964).abs() < 1e-5);
        assert!((normal_cdf_inv(0.025) + 1.959_964).abs() < 1e-5);
    }

    #[test]
    fn rejects_invalid_definitions() {
        assert!(Parameter::uniform("x", 5.0, 5.0).is_err());
        assert!(Parameter::uniform("x", f64::NAN, 1.0).is_err());
        assert!(Parameter::gaussian("x", 0.0, 1.0, 0.5, 0.0).is_err());
        assert!(Parameter::gaussian("x", 0.0, 1.0, f64::NAN, 1.0).is_err());
        assert!(param_uniform_scoped().with_scope(3.0, 3.0).is_err());
        // scope outside the definition range
        let err = Parameter::uniform("x", 0.0, 10.0)
            .unwrap()
            .with_scope(5.0, 11.0);
        assert!(err.is_err());
    }

    #[test]
    fn in_scope_bounds_inclusive() {
        let p = param_uniform_scoped();
        assert!(p.in_scope(2.0));
        assert!(p.in_scope(8.0));
        assert!(p.in_scope(5.0));
        assert!(!p.in_scope(1.999));
        assert!(!p.in_scope(8.001));
    }

    #[test]
    fn full_surface_overrides_distribution_and_scope() {
        let p = param_gaussian_scoped().full_surface();
        assert_eq!(p.distribution, Distribution::Uniform);
        assert_eq!(p.scope_min, p.min);
        assert_eq!(p.scope_max, p.max);
        assert!(p.in_scope(p.min));
        assert!(p.in_scope(p.max));
        // the transform is now the plain affine full-range map
        assert!(p.transform(0.0).abs() < 1e-12);
        assert!((p.transform(10.0) - 1.0).abs() < 1e-12);
        assert!((p.transform(2.5) - 0.25).abs() < 1e-12);
        // the original parameter is untouched
        let original = param_gaussian_scoped();
        assert_eq!(original.scope_min, 3.0);
        assert_eq!(
            original.distribution,
            Distribution::Gaussian {
                mean: 5.0,
                sigma: 2.0
            }
        );
    }
}
