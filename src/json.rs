//! JSON (de)serialization for the `slhs` file format.
//!
//! The whole document is handled as a `serde_json::Value` so that unknown
//! fields and key order are preserved verbatim; only the fields understood
//! by the tool are extracted into the typed model.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Number, Value};

use crate::Design;
use crate::error::{Error, Result};
use crate::scaling::Parameter;

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
