//! JSON (de)serialization for the `plhs` file format.
//!
//! The whole document is handled as a `serde_json::Value` so that unknown
//! fields and key order are preserved verbatim; only the fields understood
//! by the tool are extracted into the typed model.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Number, Value};

use doe_types::error::{Error, Result};
use doe_types::{Design, Parameter};

use doe_types::Results;

fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

fn object<'v>(value: &'v Value, what: &str) -> Result<&'v Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| invalid(format!("'{what}' must be a JSON object")))
}

fn require<'v>(map: &'v Map<String, Value>, key: &str, what: &str) -> Result<&'v Value> {
    map.get(key)
        .ok_or_else(|| invalid(format!("'{what}' is missing the field '{key}'")))
}

fn string(map: &Map<String, Value>, key: &str, what: &str) -> Result<String> {
    require(map, key, what)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("'{what}.{key}' must be a string")))
}

fn finite_number(map: &Map<String, Value>, key: &str, what: &str) -> Result<f64> {
    let v = require(map, key, what)?;
    let n = v
        .as_f64()
        .ok_or_else(|| invalid(format!("'{what}.{key}' must be a number")))?;
    if !n.is_finite() {
        return Err(invalid(format!(
            "'{what}.{key}' must be finite (no NaN or infinity)"
        )));
    }
    Ok(n)
}

/// Parses `params` and resolves `scope` into [`Parameter`]s.
///
/// - `params` is required and must contain at least one entry.
/// - `distribution` defaults to `uniform`; `gaussian`/`normal` require
///   `mean` (alias `nom`) and `sigma`.
/// - When `scope` is absent it defaults to a copy of `params`; when present,
///   it must contain exactly one `min`/`max` entry per declared parameter.
pub fn parse_params(root: &Value) -> Result<Vec<Parameter>> {
    let root_obj = object(root, "root")?;
    let params_v = require(root_obj, "params", "root")?;
    let params_arr = params_v
        .as_array()
        .ok_or_else(|| invalid("'params' must be an array"))?;
    if params_arr.is_empty() {
        return Err(invalid("'params' must contain at least one parameter"));
    }

    // Scope entries, if the scope is present.
    let mut scope: Option<BTreeMap<String, (f64, f64)>> = None;
    if let Some(scope_v) = root_obj.get("scope") {
        let arr = scope_v
            .as_array()
            .ok_or_else(|| invalid("'scope' must be an array"))?;
        let mut map = BTreeMap::new();
        for entry in arr {
            let obj = object(entry, "scope entry")?;
            let name = string(obj, "name", "scope entry")?;
            let min = finite_number(obj, "min", "scope entry")?;
            let max = finite_number(obj, "max", "scope entry")?;
            if map.insert(name.clone(), (min, max)).is_some() {
                return Err(Error::ScopeEntry {
                    name,
                    reason: "duplicate scope entry".to_owned(),
                });
            }
        }
        scope = Some(map);
    }

    let mut params = Vec::with_capacity(params_arr.len());
    let mut seen = BTreeSet::new();
    for entry in params_arr {
        let obj = object(entry, "parameter")?;
        let name = string(obj, "name", "parameter")?;
        if !seen.insert(name.clone()) {
            return Err(Error::Parameter {
                name,
                reason: "duplicate parameter name".to_owned(),
            });
        }
        let min = finite_number(obj, "min", "parameter")?;
        let max = finite_number(obj, "max", "parameter")?;

        let param = match obj.get("distribution") {
            None => Parameter::uniform(&name, min, max)?,
            Some(v) => match v.as_str() {
                Some("uniform") => Parameter::uniform(&name, min, max)?,
                Some("gaussian") | Some("normal") => {
                    let mean = match obj.get("mean").or_else(|| obj.get("nom")) {
                        Some(m) => {
                            let m = m.as_f64().ok_or_else(|| Error::Parameter {
                                name: name.clone(),
                                reason: "'mean' (alias 'nom') must be a number".to_owned(),
                            })?;
                            if !m.is_finite() {
                                return Err(Error::Parameter {
                                    name: name.clone(),
                                    reason: "'mean' must be finite".to_owned(),
                                });
                            }
                            m
                        }
                        None => {
                            return Err(Error::Parameter {
                                name,
                                reason: "gaussian distribution requires 'mean' (alias 'nom')"
                                    .to_owned(),
                            });
                        }
                    };
                    let sigma = match obj.get("sigma") {
                        Some(s) => {
                            let s = s.as_f64().ok_or_else(|| Error::Parameter {
                                name: name.clone(),
                                reason: "'sigma' must be a number".to_owned(),
                            })?;
                            if !s.is_finite() {
                                return Err(Error::Parameter {
                                    name: name.clone(),
                                    reason: "'sigma' must be finite".to_owned(),
                                });
                            }
                            s
                        }
                        None => {
                            return Err(Error::Parameter {
                                name,
                                reason: "gaussian distribution requires 'sigma'".to_owned(),
                            });
                        }
                    };
                    Parameter::gaussian(&name, min, max, mean, sigma)?
                }
                Some(other) => {
                    return Err(Error::Parameter {
                        name,
                        reason: format!(
                            "unsupported distribution '{other}' (expected 'uniform' or 'gaussian')"
                        ),
                    });
                }
                None => {
                    return Err(Error::Parameter {
                        name,
                        reason: "'distribution' must be a string".to_owned(),
                    });
                }
            },
        };

        let param = match &scope {
            None => param,
            Some(map) => {
                let (smin, smax) = map
                    .get(&name)
                    .ok_or_else(|| Error::MissingScopeEntry(name.clone()))?;
                param.with_scope(*smin, *smax)?
            }
        };
        params.push(param);
    }

    // Every scope entry must belong to a declared parameter.
    if let Some(map) = &scope {
        for name in map.keys() {
            if !seen.contains(name) {
                return Err(Error::ScopeEntry {
                    name: name.clone(),
                    reason: "unknown parameter".to_owned(),
                });
            }
        }
    }

    Ok(params)
}

/// Parses `designs` into aligned [`Design`]s. Every design must have a
/// `name` and a finite numeric value for every declared parameter.
pub fn parse_designs(root: &Value, params: &[Parameter]) -> Result<Vec<Design>> {
    let root_obj = object(root, "root")?;
    let designs_v = require(root_obj, "designs", "root")?;
    let arr = designs_v
        .as_array()
        .ok_or_else(|| invalid("'designs' must be an array"))?;
    let mut designs = Vec::with_capacity(arr.len());
    for entry in arr {
        let obj = object(entry, "design")?;
        let name = string(obj, "name", "design")?;
        let mut values = Vec::with_capacity(params.len());
        for param in params {
            let v = match obj.get(&param.name) {
                Some(v) => v,
                None => {
                    return Err(Error::Design {
                        name,
                        reason: format!("missing value for parameter '{}'", param.name),
                    });
                }
            };
            let n = v.as_f64().ok_or_else(|| Error::Design {
                name: name.clone(),
                reason: format!("value for parameter '{}' must be a number", param.name),
            })?;
            if !n.is_finite() {
                return Err(Error::Design {
                    name,
                    reason: format!("value for parameter '{}' must be finite", param.name),
                });
            }
            values.push(n);
        }
        designs.push(Design { name, values });
    }
    Ok(designs)
}

/// Appends the new designs to `designs` and overwrites `new_designs` with
/// the new batch. All other fields (including `results`) are left untouched.
pub fn append_new_designs(root: &mut Value, params: &[Parameter], new: &[Design]) -> Result<()> {
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| invalid("root must be a JSON object"))?;
    let designs = root_obj
        .get_mut("designs")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("'designs' must be an array"))?;
    let mut new_arr = Vec::with_capacity(new.len());
    for design in new {
        let mut obj = Map::new();
        obj.insert("name".to_owned(), Value::String(design.name.clone()));
        for (param, value) in params.iter().zip(&design.values) {
            let n = Number::from_f64(*value).ok_or_else(|| {
                invalid(format!(
                    "design '{}': non-finite value for parameter '{}'",
                    design.name, param.name
                ))
            })?;
            obj.insert(param.name.clone(), Value::Number(n));
        }
        let v = Value::Object(obj);
        designs.push(v.clone());
        new_arr.push(v);
    }
    root_obj.insert("new_designs".to_owned(), Value::Array(new_arr));
    Ok(())
}

/// Parses `results` entries into typed [`Results`].
///
/// Each entry must be an object with a `name`; every other numeric field
/// becomes a result variable. Non-numeric fields are not results and are
/// ignored. A missing `results` key is an empty list — the caller decides
/// whether that is an error.
pub fn parse_results(root: &Value) -> Result<Vec<Results>> {
    let root_obj = object(root, "root")?;
    let entries: Vec<&Value> = match root_obj.get("results") {
        None => Vec::new(),
        Some(Value::Array(a)) => a.iter().collect(),
        Some(_) => return Err(invalid("'root.results' must be an array")),
    };
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let obj = object(entry, "results entry")?;
        let name = string(obj, "name", "results entry")?;
        let mut values = BTreeMap::new();
        for (key, v) in obj.iter() {
            if key == "name" {
                continue;
            }
            if let Some(n) = v.as_f64() {
                if n.is_finite() {
                    values.insert(key.clone(), n);
                }
            }
        }
        out.push(Results { name, values });
    }
    Ok(out)
}

/// Outcome of a pool reset ([`reset_designs`]): the design and results
/// counts before and after the removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetReport {
    /// Designs in the pool before the reset.
    pub designs_before: usize,
    /// Designs in the pool after the reset.
    pub designs_after: usize,
    /// Results entries before the reset.
    pub results_before: usize,
    /// Results entries after the reset (unchanged unless `all`).
    pub results_after: usize,
}

/// Removes designs from the pool in place and reports the counts.
///
/// With `all = false` every design whose name has no results entry is
/// removed — the same name-based solved-set predicate as
/// [`append_results`]; evaluated designs and all results entries are
/// kept untouched.
///
/// With `all = true` every design is removed together with the whole
/// `results` array and the stale `new_designs` batch record: the pool
/// returns to its initial state (parameters only, empty `designs`),
/// so the next augmentation starts at iteration 1 again.
///
/// Unknown fields and key order are preserved. A missing `results` key
/// counts as zero entries and never fails; a `designs` key that is
/// missing or not an array is a validation error.
pub fn reset_designs(root: &mut Value, all: bool) -> Result<ResetReport> {
    let results = parse_results(root)?;
    let solved: BTreeSet<&str> = results.iter().map(|r| r.name.as_str()).collect();
    let results_before = results.len();

    let designs_not_array = || invalid("'designs' must be an array");
    let designs_before = match root.get("designs") {
        Some(Value::Array(a)) => a.len(),
        Some(_) => return Err(designs_not_array()),
        None => return Err(invalid("'root' is missing the field 'designs'")),
    };

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| invalid("root must be a JSON object"))?;
    let designs = root_obj
        .get_mut("designs")
        .and_then(Value::as_array_mut)
        .ok_or_else(designs_not_array)?;

    let report = if all {
        designs.clear();
        root_obj.remove("results");
        root_obj.remove("new_designs");
        ResetReport {
            designs_before,
            designs_after: 0,
            results_before,
            results_after: 0,
        }
    } else {
        designs.retain(|d| {
            d.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| solved.contains(name))
        });
        ResetReport {
            designs_before,
            designs_after: designs.len(),
            results_before,
            results_after: results_before,
        }
    };
    Ok(report)
}

/// Appends evaluated `results` entries to the pool's `results` array.
///
/// Every entry must name a design that exists in the pool and must not
/// collide with an existing results entry (one entry per design; which
/// designs count as solved is the caller's bookkeeping). The `results`
/// array is created when the pool has none yet. All other fields are
/// left untouched.
pub fn append_results(root: &mut Value, params: &[Parameter], results: &[Results]) -> Result<()> {
    let designs = parse_designs(root, params)?;
    let existing = parse_results(root)?;
    let design_names: BTreeSet<&str> = designs.iter().map(|d| d.name.as_str()).collect();
    let existing_names: BTreeSet<&str> = existing.iter().map(|r| r.name.as_str()).collect();
    for r in results {
        if !design_names.contains(r.name.as_str()) {
            return Err(Error::Design {
                name: r.name.clone(),
                reason: "results entry does not match any design".to_owned(),
            });
        }
        if existing_names.contains(r.name.as_str()) {
            return Err(Error::Design {
                name: r.name.clone(),
                reason: "a results entry for this design already exists".to_owned(),
            });
        }
    }
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| invalid("root must be a JSON object"))?;
    if !root_obj.contains_key("results") {
        root_obj.insert("results".to_owned(), Value::Array(Vec::new()));
    }
    let arr = root_obj
        .get_mut("results")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("'root.results' must be an array"))?;
    for result in results {
        let mut obj = Map::new();
        obj.insert("name".to_owned(), Value::String(result.name.clone()));
        for (key, value) in &result.values {
            let n = Number::from_f64(*value).ok_or_else(|| Error::Design {
                name: result.name.clone(),
                reason: format!("non-finite value for result variable '{key}'"),
            })?;
            obj.insert(key.clone(), Value::Number(n));
        }
        arr.push(Value::Object(obj));
    }
    Ok(())
}

/// Updates the parameter values of the named designs in place (the
/// `sampling polish` subcommand writes its refined values back through
/// this). Every updated design must exist in the pool; only the
/// per-parameter value fields change — names, other designs, `results`
/// and unknown fields are untouched, and the key order is preserved.
pub fn update_design_values(
    root: &mut Value,
    params: &[Parameter],
    updated: &[Design],
) -> Result<()> {
    let designs_not_array = || invalid("'designs' must be an array");
    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| invalid("root must be a JSON object"))?;
    let designs = root_obj
        .get_mut("designs")
        .and_then(Value::as_array_mut)
        .ok_or_else(designs_not_array)?;
    for design in updated {
        let entry = designs
            .iter_mut()
            .find(|d| d.get("name").and_then(Value::as_str) == Some(design.name.as_str()))
            .ok_or_else(|| Error::Design {
                name: design.name.clone(),
                reason: "does not match any design in the pool".to_owned(),
            })?;
        let obj = entry
            .as_object_mut()
            .ok_or_else(|| invalid("design must be an object"))?;
        for (param, value) in params.iter().zip(&design.values) {
            let n = Number::from_f64(*value).ok_or_else(|| Error::Design {
                name: design.name.clone(),
                reason: format!("non-finite value for parameter '{}'", param.name),
            })?;
            obj.insert(param.name.clone(), Value::Number(n));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal pool: one uniform parameter `a` and designs `1.1`, `1.2`.
    fn pool() -> Value {
        serde_json::json!({
            "params": [{ "name": "a", "min": 0, "max": 10 }],
            "designs": [
                { "name": "1.1", "a": 2.0 },
                { "name": "1.2", "a": 3.0 }
            ],
            "note": "unknown fields must survive"
        })
    }

    fn one_result(name: &str, value: f64) -> Results {
        Results {
            name: name.to_owned(),
            values: BTreeMap::from([("linear".to_owned(), value)]),
        }
    }

    #[test]
    fn append_results_creates_the_array_and_keeps_unknown_fields() {
        let mut root = pool();
        let params = parse_params(&root).unwrap();
        append_results(&mut root, &params, &[one_result("1.1", 4.2)]).unwrap();
        let entry = &root["results"][0];
        assert_eq!(entry["name"], "1.1");
        assert_eq!(entry["linear"], 4.2);
        assert_eq!(root["note"], "unknown fields must survive");
        assert_eq!(root["designs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn append_results_appends_after_existing_entries() {
        let mut root = pool();
        root["results"] = serde_json::json!([{ "name": "1.1", "linear": 1.0 }]);
        let params = parse_params(&root).unwrap();
        append_results(&mut root, &params, &[one_result("1.2", 2.0)]).unwrap();
        assert_eq!(root["results"].as_array().unwrap().len(), 2);
        assert_eq!(root["results"][1]["name"], "1.2");
        assert_eq!(root["results"][0]["linear"], 1.0);
    }

    #[test]
    fn append_results_refuses_a_name_matching_no_design() {
        let mut root = pool();
        let params = parse_params(&root).unwrap();
        let err = append_results(&mut root, &params, &[one_result("9.9", 1.0)]).unwrap_err();
        assert!(err.to_string().contains("9.9"), "{err}");
        assert!(
            err.to_string().contains("does not match any design"),
            "{err}"
        );
    }

    #[test]
    fn append_results_refuses_a_duplicate_entry() {
        let mut root = pool();
        root["results"] = serde_json::json!([{ "name": "1.1", "linear": 1.0 }]);
        let params = parse_params(&root).unwrap();
        let err = append_results(&mut root, &params, &[one_result("1.1", 2.0)]).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[test]
    fn reset_removes_designs_without_a_results_entry() {
        let mut root = pool();
        root["results"] =
            serde_json::json!([{ "name": "1.1", "linear": 1.0 }, { "name": "2.9", "linear": 2.0 }]);
        let report = reset_designs(&mut root, false).unwrap();
        assert_eq!(
            report,
            ResetReport {
                designs_before: 2,
                designs_after: 1,
                results_before: 2,
                results_after: 2,
            }
        );
        let names: Vec<&str> = root["designs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["1.1"], "only the evaluated design survives");
        // an entry naming a design that does not exist is not the
        // reset's business: results pass through untouched
        assert_eq!(root["results"].as_array().unwrap().len(), 2);
        assert_eq!(root["note"], "unknown fields must survive");
    }

    #[test]
    fn reset_without_results_removes_every_design() {
        let mut root = pool();
        let report = reset_designs(&mut root, false).unwrap();
        assert_eq!(report.designs_before, 2);
        assert_eq!(report.designs_after, 0);
        assert_eq!(report.results_before, 0);
        assert_eq!(report.results_after, 0);
        assert!(root["designs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn reset_all_returns_the_pool_to_its_initial_state() {
        let mut root = pool();
        root["new_designs"] = serde_json::json!([{ "name": "2.1", "a": 9.0 }]);
        root["results"] = serde_json::json!([{ "name": "1.1", "linear": 1.0 }]);
        let report = reset_designs(&mut root, true).unwrap();
        assert_eq!(
            report,
            ResetReport {
                designs_before: 2,
                designs_after: 0,
                results_before: 1,
                results_after: 0,
            }
        );
        assert!(root["designs"].as_array().unwrap().is_empty());
        assert!(root.get("results").is_none(), "the results array is gone");
        assert!(
            root.get("new_designs").is_none(),
            "the stale batch record is gone"
        );
        assert_eq!(root["note"], "unknown fields must survive");
    }

    #[test]
    fn reset_all_on_a_pool_without_results_is_a_no_op_report() {
        let mut root = pool();
        let report = reset_designs(&mut root, true).unwrap();
        assert_eq!(report.designs_before, 2);
        assert_eq!(report.results_before, 0);
        assert!(root["designs"].as_array().unwrap().is_empty());
        assert!(root.get("results").is_none(), "no results key is created");
    }

    #[test]
    fn update_design_values_rewrites_only_the_named_designs() {
        let mut root = serde_json::json!({
            "params": [
                { "name": "a", "min": 0, "max": 10 },
                { "name": "b", "min": 0, "max": 10 }
            ],
            "designs": [
                { "name": "1.1", "a": 2.0, "b": 8.0, "note": "kept" },
                { "name": "1.2", "a": 3.0, "b": 7.0 }
            ],
            "results": [{ "name": "1.2", "linear": 1.0 }],
            "note": "unknown fields must survive"
        });
        let params = parse_params(&root).unwrap();
        update_design_values(
            &mut root,
            &params,
            &[Design {
                name: "1.1".to_owned(),
                values: vec![4.5, 5.5],
            }],
        )
        .unwrap();
        assert_eq!(root["designs"][0]["a"], 4.5);
        assert_eq!(root["designs"][0]["b"], 5.5);
        assert_eq!(root["designs"][0]["note"], "kept");
        assert_eq!(root["designs"][0]["name"], "1.1");
        // the other design and everything else pass through untouched
        assert_eq!(root["designs"][1]["a"], 3.0);
        assert_eq!(root["designs"][1]["b"], 7.0);
        assert_eq!(root["results"].as_array().unwrap().len(), 1);
        assert_eq!(root["note"], "unknown fields must survive");
    }

    #[test]
    fn update_design_values_refuses_a_name_matching_no_design() {
        let mut root = pool();
        let params = parse_params(&root).unwrap();
        let err = update_design_values(
            &mut root,
            &params,
            &[Design {
                name: "9.9".to_owned(),
                values: vec![1.0],
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("9.9"), "{err}");
        assert!(err.to_string().contains("does not match"), "{err}");
    }

    #[test]
    fn update_design_values_preserves_key_order() {
        let mut root = serde_json::json!({
            "params": [{ "name": "a", "min": 0, "max": 10 }],
            "designs": [{ "name": "1.1", "a": 2.0 }]
        });
        let params = parse_params(&root).unwrap();
        update_design_values(
            &mut root,
            &params,
            &[Design {
                name: "1.1".to_owned(),
                values: vec![6.0],
            }],
        )
        .unwrap();
        // serialized form keeps name before the value fields
        let text = root.to_string();
        assert_eq!(
            text,
            r#"{"params":[{"name":"a","min":0,"max":10}],"designs":[{"name":"1.1","a":6.0}]}"#
        );
    }
}
