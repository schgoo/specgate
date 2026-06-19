//! Spec YAML parsing.
//!
//! The harness spec uses a list-of-maps `expected:` shape that is
//! incompatible with `specgate-types::SpecDocument`, so we parse with
//! `serde_yaml::Value` and pull out the fields we care about by hand.

use crate::types::{Assertion, CaseLevel, Source};
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Spec {
    pub binding_path: Option<String>,
    pub cases: Vec<Case>,
    /// Names of operations declared `async: true` in the spec.
    pub async_ops: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub setup: Setup,
    pub operation: Option<String>,
    pub steps: Vec<String>,
    pub inputs: BTreeMap<String, Value>,
    pub expected: Vec<Assertion>,
    pub level: CaseLevel,
    pub source: Option<Source>,
}

#[derive(Debug, Clone)]
pub enum Setup {
    None,
    Single(String),
    Multi(Vec<(String, String)>), // alias → setup_fn
}

#[derive(Debug)]
pub enum ParseError {
    #[allow(dead_code)]
    Io(String),
    #[allow(dead_code)]
    Yaml(String),
    #[allow(dead_code)]
    Shape(String),
}

#[allow(dead_code)]
pub fn load_spec(path: &Path) -> Result<Spec, ParseError> {
    let text = std::fs::read_to_string(path).map_err(|e| ParseError::Io(e.to_string()))?;
    let v: Value = serde_yaml::from_str(&text).map_err(|e| ParseError::Yaml(e.to_string()))?;
    parse_spec_value(&v)
}

pub fn parse_value(v: &Value) -> Result<Spec, ParseError> {
    parse_spec_value(v)
}

fn parse_spec_value(v: &Value) -> Result<Spec, ParseError> {
    let map = v
        .as_mapping()
        .ok_or_else(|| ParseError::Shape("top-level is not a mapping".into()))?;

    let binding_path = map
        .get(Value::String("binding".into()))
        .and_then(|b| b.as_str())
        .map(String::from);

    let mut async_ops = BTreeSet::new();
    if let Some(Value::Mapping(ops)) = map.get(Value::String("operations".into())) {
        for (k, v) in ops {
            let Some(name) = k.as_str() else { continue };
            let Some(body) = v.as_mapping() else { continue };
            if let Some(a) = body.get(Value::String("async".into())) {
                if a.as_bool() == Some(true) {
                    async_ops.insert(name.to_string());
                }
            }
        }
    }

    let cases_v = map
        .get(Value::String("cases".into()))
        .ok_or_else(|| ParseError::Shape("missing field: cases".into()))?;
    let cases_seq = cases_v
        .as_sequence()
        .ok_or_else(|| ParseError::Shape("cases is not a sequence".into()))?;

    let mut cases = Vec::new();
    for c in cases_seq {
        cases.push(parse_case(c)?);
    }
    Ok(Spec {
        binding_path,
        cases,
        async_ops,
    })
}

fn parse_case(v: &Value) -> Result<Case, ParseError> {
    let m = v
        .as_mapping()
        .ok_or_else(|| ParseError::Shape("case is not a mapping".into()))?;
    let name = m
        .get(Value::String("name".into()))
        .and_then(|x| x.as_str())
        .ok_or_else(|| ParseError::Shape("case missing name".into()))?
        .to_string();

    let mut extra_inputs: BTreeMap<String, Value> = BTreeMap::new();
    let setup = match m.get(Value::String("setup".into())) {
        None => Setup::None,
        Some(Value::String(s)) => Setup::Single(s.clone()),
        Some(Value::Mapping(mp)) => {
            let all_strings = mp.iter().all(|(_, v)| v.as_str().is_some());
            if all_strings {
                let mut entries = Vec::new();
                for (k, val) in mp {
                    let alias = k
                        .as_str()
                        .ok_or_else(|| ParseError::Shape("setup alias not str".into()))?
                        .to_string();
                    let fn_name = val.as_str().unwrap().to_string();
                    entries.push((alias, fn_name));
                }
                Setup::Multi(entries)
            } else {
                let (k, v) = mp
                    .iter()
                    .next()
                    .ok_or_else(|| ParseError::Shape("empty setup mapping".into()))?;
                let fn_name = k
                    .as_str()
                    .ok_or_else(|| ParseError::Shape("setup name not str".into()))?
                    .to_string();
                if let Value::Mapping(pm) = v {
                    for (pk, pv) in pm {
                        if let Some(pks) = pk.as_str() {
                            extra_inputs.insert(pks.to_string(), pv.clone());
                        }
                    }
                }
                Setup::Single(fn_name)
            }
        }
        Some(_) => return Err(ParseError::Shape("setup has invalid shape".into())),
    };

    let operation = m
        .get(Value::String("operation".into()))
        .and_then(|x| x.as_str())
        .map(String::from);

    let steps = match m.get(Value::String("steps".into())) {
        None => Vec::new(),
        Some(Value::Sequence(seq)) => {
            let mut out = Vec::new();
            for s in seq {
                let m = s
                    .as_mapping()
                    .ok_or_else(|| ParseError::Shape("step not a mapping".into()))?;
                let op = m
                    .get(Value::String("operation".into()))
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| ParseError::Shape("step missing operation".into()))?
                    .to_string();
                out.push(op);
            }
            out
        }
        Some(_) => return Err(ParseError::Shape("steps has invalid shape".into())),
    };

    let inputs = match m.get(Value::String("inputs".into())) {
        None => BTreeMap::new(),
        Some(Value::Mapping(mp)) => {
            let mut out = BTreeMap::new();
            for (k, val) in mp {
                let key = k
                    .as_str()
                    .ok_or_else(|| ParseError::Shape("input key not string".into()))?
                    .to_string();
                out.insert(key, val.clone());
            }
            out
        }
        Some(_) => return Err(ParseError::Shape("inputs has invalid shape".into())),
    };
    let mut inputs = inputs;
    for (k, v) in extra_inputs {
        inputs.entry(k).or_insert(v);
    }

    let expected = match m.get(Value::String("expected".into())) {
        None => Vec::new(),
        Some(Value::Sequence(seq)) => parse_assertion_list(seq)?,
        // Legacy / non-list `expected:` shapes are tolerated as empty.
        Some(_) => Vec::new(),
    };

    let level = match m.get(Value::String("level".into())) {
        None => CaseLevel::Must,
        Some(Value::String(s)) => match s.as_str() {
            "must" => CaseLevel::Must,
            "should" => CaseLevel::Should,
            "may" => CaseLevel::May,
            other => {
                return Err(ParseError::Shape(format!("invalid level value: {other}")));
            }
        },
        Some(_) => return Err(ParseError::Shape("level has invalid shape".into())),
    };

    let source = match m.get(Value::String("source".into())) {
        None => None,
        Some(Value::Mapping(sm)) => {
            let mut s = Source::default();
            if let Some(Value::Sequence(ids)) = sm.get(Value::String("assertion_ids".into())) {
                for id in ids {
                    if let Some(t) = id.as_str() {
                        s.assertion_ids.push(t.to_string());
                    }
                }
            }
            if let Some(Value::String(t)) = sm.get(Value::String("spec".into())) {
                s.spec = t.clone();
            }
            if let Some(Value::String(t)) = sm.get(Value::String("section".into())) {
                s.section = t.clone();
            }
            Some(s)
        }
        Some(_) => return Err(ParseError::Shape("source has invalid shape".into())),
    };

    Ok(Case {
        name,
        setup,
        operation,
        steps,
        inputs,
        expected,
        level,
        source,
    })
}

fn parse_assertion_list(seq: &[Value]) -> Result<Vec<Assertion>, ParseError> {
    let mut out = Vec::new();
    for entry in seq {
        out.push(parse_assertion(entry)?);
    }
    Ok(out)
}

fn parse_assertion(v: &Value) -> Result<Assertion, ParseError> {
    let m = v
        .as_mapping()
        .ok_or_else(|| ParseError::Shape("assertion is not a mapping".into()))?;
    if m.len() != 1 {
        return Err(ParseError::Shape(
            "assertion entry must be a single-key mapping".into(),
        ));
    }
    let (k, val) = m.iter().next().unwrap();
    let key = k
        .as_str()
        .ok_or_else(|| ParseError::Shape("assertion key not string".into()))?;
    match key {
        "$run" => {
            let op = val
                .as_str()
                .ok_or_else(|| ParseError::Shape("$run value not string".into()))?
                .to_string();
            Ok(Assertion::Run { operation: op })
        }
        "$unordered" => {
            let seq = val
                .as_sequence()
                .ok_or_else(|| ParseError::Shape("$unordered value not a sequence".into()))?;
            Ok(Assertion::Unordered {
                items: parse_assertion_list(seq)?,
            })
        }
        "$anywhere" => {
            let seq = val
                .as_sequence()
                .ok_or_else(|| ParseError::Shape("$anywhere value not a sequence".into()))?;
            Ok(Assertion::Anywhere {
                items: parse_assertion_list(seq)?,
            })
        }
        other => Ok(Assertion::Event {
            name: other.to_string(),
            value: stringify_value(val),
        }),
    }
}

pub fn stringify_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Sequence(s) => {
            let parts: Vec<String> = s.iter().map(stringify_value).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Mapping(_) => "<map>".into(),
        Value::Tagged(t) => stringify_value(&t.value),
    }
}

pub fn binding_path_resolved(spec_path: &Path, binding: &str) -> PathBuf {
    let parent = spec_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    // Try spec-relative first.
    let direct = parent.join(binding);
    if direct.exists() {
        return direct;
    }
    // Walk up parent directories and try each.
    let mut cur = parent.as_path();
    while let Some(p) = cur.parent() {
        let candidate = p.join(binding);
        if candidate.exists() {
            return candidate;
        }
        cur = p;
    }
    // Fall back to the spec-relative path (will surface as "not found").
    direct
}
