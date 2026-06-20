//! Spec YAML parsing.
//!
//! The harness spec uses a list-of-maps `expected:` shape that is
//! incompatible with `specgate-types::SpecDocument`, so we parse with
//! `serde_yaml::YValue` and pull out the fields we care about by hand.

use crate::types::{AnyArg, AssertValue, Assertion, CaseLevel, Matcher, Source, Value};
use serde_yaml::Value as YValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Spec {
    pub binding_path: Option<String>,
    pub target: Option<String>,
    pub cases: Vec<Case>,
    /// Names of operations declared `async: true` in the spec.
    pub async_ops: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    /// Case-level target override (overrides the spec-level `target`).
    pub target: Option<String>,
    pub setup: Setup,
    pub operation: Option<String>,
    pub steps: Vec<String>,
    pub inputs: BTreeMap<String, YValue>,
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
    let v: YValue = serde_yaml::from_str(&text).map_err(|e| ParseError::Yaml(e.to_string()))?;
    parse_spec_value(&v)
}

pub fn parse_spec(v: &YValue) -> Result<Spec, ParseError> {
    parse_spec_value(v)
}

fn parse_spec_value(v: &YValue) -> Result<Spec, ParseError> {
    let map = v
        .as_mapping()
        .ok_or_else(|| ParseError::Shape("top-level is not a mapping".into()))?;

    let binding_path = map.get(YValue::String("binding".into())).and_then(|b| b.as_str()).map(String::from);

    let target = map.get(YValue::String("target".into())).and_then(|t| t.as_str()).map(String::from);

    let mut async_ops = BTreeSet::new();
    if let Some(YValue::Mapping(ops)) = map.get(YValue::String("operations".into())) {
        for (k, v) in ops {
            let Some(name) = k.as_str() else { continue };
            let Some(body) = v.as_mapping() else { continue };
            if let Some(a) = body.get(YValue::String("async".into())) {
                if a.as_bool() == Some(true) {
                    async_ops.insert(name.to_string());
                }
            }
        }
    }

    let cases_v = map
        .get(YValue::String("cases".into()))
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
        target,
        cases,
        async_ops,
    })
}

fn parse_case(v: &YValue) -> Result<Case, ParseError> {
    let m = v.as_mapping().ok_or_else(|| ParseError::Shape("case is not a mapping".into()))?;
    let name = m
        .get(YValue::String("name".into()))
        .and_then(|x| x.as_str())
        .ok_or_else(|| ParseError::Shape("case missing name".into()))?
        .to_string();

    let target = m.get(YValue::String("target".into())).and_then(|t| t.as_str()).map(String::from);

    let mut extra_inputs: BTreeMap<String, YValue> = BTreeMap::new();
    let setup = match m.get(YValue::String("setup".into())) {
        None => Setup::None,
        Some(YValue::String(s)) => Setup::Single(s.clone()),
        Some(YValue::Mapping(mp)) => {
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
                let (k, v) = mp.iter().next().ok_or_else(|| ParseError::Shape("empty setup mapping".into()))?;
                let fn_name = k
                    .as_str()
                    .ok_or_else(|| ParseError::Shape("setup name not str".into()))?
                    .to_string();
                if let YValue::Mapping(pm) = v {
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

    let operation = m.get(YValue::String("operation".into())).and_then(|x| x.as_str()).map(String::from);

    let steps = match m.get(YValue::String("steps".into())) {
        None => Vec::new(),
        Some(YValue::Sequence(seq)) => {
            let mut out = Vec::new();
            for s in seq {
                let m = s.as_mapping().ok_or_else(|| ParseError::Shape("step not a mapping".into()))?;
                let op = m
                    .get(YValue::String("operation".into()))
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| ParseError::Shape("step missing operation".into()))?
                    .to_string();
                out.push(op);
            }
            out
        }
        Some(_) => return Err(ParseError::Shape("steps has invalid shape".into())),
    };

    let inputs = match m.get(YValue::String("inputs".into())) {
        None => BTreeMap::new(),
        Some(YValue::Mapping(mp)) => {
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

    let expected = match m.get(YValue::String("expected".into())) {
        None => Vec::new(),
        Some(YValue::Sequence(seq)) => parse_assertion_list(seq)?,
        // Legacy / non-list `expected:` shapes are tolerated as empty.
        Some(_) => Vec::new(),
    };

    let level = match m.get(YValue::String("level".into())) {
        None => CaseLevel::Must,
        Some(YValue::String(s)) => match s.as_str() {
            "must" => CaseLevel::Must,
            "should" => CaseLevel::Should,
            "may" => CaseLevel::May,
            other => {
                return Err(ParseError::Shape(format!("invalid level value: {other}")));
            }
        },
        Some(_) => return Err(ParseError::Shape("level has invalid shape".into())),
    };

    let source = match m.get(YValue::String("source".into())) {
        None => None,
        Some(YValue::Mapping(sm)) => {
            let mut s = Source::default();
            if let Some(YValue::Sequence(ids)) = sm.get(YValue::String("assertion_ids".into())) {
                for id in ids {
                    if let Some(t) = id.as_str() {
                        s.assertion_ids.push(t.to_string());
                    }
                }
            }
            if let Some(YValue::String(t)) = sm.get(YValue::String("spec".into())) {
                s.spec = t.clone();
            }
            if let Some(YValue::String(t)) = sm.get(YValue::String("section".into())) {
                s.section = t.clone();
            }
            Some(s)
        }
        Some(_) => return Err(ParseError::Shape("source has invalid shape".into())),
    };

    Ok(Case {
        name,
        target,
        setup,
        operation,
        steps,
        inputs,
        expected,
        level,
        source,
    })
}

fn parse_assertion_list(seq: &[YValue]) -> Result<Vec<Assertion>, ParseError> {
    let mut out = Vec::new();
    for entry in seq {
        out.push(parse_assertion(entry)?);
    }
    Ok(out)
}

fn parse_assertion(v: &YValue) -> Result<Assertion, ParseError> {
    let m = v
        .as_mapping()
        .ok_or_else(|| ParseError::Shape("assertion is not a mapping".into()))?;
    if m.len() != 1 {
        return Err(ParseError::Shape("assertion entry must be a single-key mapping".into()));
    }
    let (k, val) = m.iter().next().unwrap();
    let key = k.as_str().ok_or_else(|| ParseError::Shape("assertion key not string".into()))?;
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
            value: parse_assert_value(val)?,
        }),
    }
}

/// Parse a YAML node into either a concrete `Value` or a `Matcher` payload.
fn parse_assert_value(v: &YValue) -> Result<AssertValue, ParseError> {
    if let YValue::Mapping(m) = v {
        let mut has_op = false;
        for (k, _) in m {
            if let Some(s) = k.as_str() {
                if s.starts_with('$') {
                    has_op = true;
                    break;
                }
            }
        }
        if has_op {
            return Ok(AssertValue::Matcher(parse_matcher(m)?));
        }
    }
    Ok(AssertValue::Exact(parse_value(v)?))
}

/// Plain YAML → structured Value, preserving type.
pub fn parse_value(v: &YValue) -> Result<Value, ParseError> {
    match v {
        YValue::String(s) => Ok(Value::String(s.clone())),
        YValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(u) = n.as_u64() {
                Ok(Value::Integer(u as i64))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                Ok(Value::String(n.to_string()))
            }
        }
        YValue::Bool(b) => Ok(Value::Bool(*b)),
        YValue::Null => Ok(Value::String(String::new())),
        YValue::Sequence(seq) => {
            let mut out = Vec::with_capacity(seq.len());
            for x in seq {
                out.push(parse_value(x)?);
            }
            Ok(Value::List(out))
        }
        YValue::Mapping(m) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in m {
                let key = k
                    .as_str()
                    .ok_or_else(|| ParseError::Shape("map key not string".into()))?
                    .to_string();
                out.insert(key, parse_value(v)?);
            }
            Ok(Value::Map(out))
        }
        YValue::Tagged(t) => parse_value(&t.value),
    }
}

fn parse_matcher(m: &serde_yaml::Mapping) -> Result<Matcher, ParseError> {
    let mut parts = Vec::new();
    for (k, v) in m {
        let key = k.as_str().ok_or_else(|| ParseError::Shape("matcher key not string".into()))?;
        let m = parse_single_op(key, v)?;
        parts.push(m);
    }
    if parts.len() == 1 {
        Ok(parts.into_iter().next().unwrap())
    } else {
        Ok(Matcher::Composite(parts))
    }
}

fn parse_single_op(op: &str, v: &YValue) -> Result<Matcher, ParseError> {
    match op {
        "$eq" => Ok(Matcher::Eq(parse_value(v)?)),
        "$size" => {
            let n = v.as_u64().ok_or_else(|| ParseError::Shape("$size expects an integer".into()))?;
            Ok(Matcher::Size(n as usize))
        }
        "$contains" => {
            let arg = if let YValue::Mapping(mp) = v {
                let has_op = mp.iter().any(|(k, _)| k.as_str().map(|s| s.starts_with('$')).unwrap_or(false));
                if has_op {
                    AnyArg::Matcher(parse_matcher(mp)?)
                } else {
                    AnyArg::Value(parse_value(v)?)
                }
            } else {
                AnyArg::Value(parse_value(v)?)
            };
            Ok(Matcher::Contains(Box::new(arg)))
        }
        "$containsAll" => {
            let seq = v
                .as_sequence()
                .ok_or_else(|| ParseError::Shape("$containsAll expects a sequence".into()))?;
            let mut items = Vec::with_capacity(seq.len());
            for x in seq {
                items.push(parse_value(x)?);
            }
            Ok(Matcher::ContainsAll(items))
        }
        "$excludes" => {
            let seq = v
                .as_sequence()
                .ok_or_else(|| ParseError::Shape("$excludes expects a sequence".into()))?;
            let mut items = Vec::with_capacity(seq.len());
            for x in seq {
                items.push(parse_value(x)?);
            }
            Ok(Matcher::Excludes(items))
        }
        "$match" => {
            let mp = v.as_mapping().ok_or_else(|| ParseError::Shape("$match expects a mapping".into()))?;
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in mp {
                let key = k
                    .as_str()
                    .ok_or_else(|| ParseError::Shape("$match key not string".into()))?
                    .to_string();
                out.insert(key, parse_value(v)?);
            }
            Ok(Matcher::Match(out))
        }
        "$exists" => {
            let b = v.as_bool().ok_or_else(|| ParseError::Shape("$exists expects a bool".into()))?;
            Ok(Matcher::Exists(b))
        }
        "$any" => {
            let arg = if let YValue::Mapping(mp) = v {
                let has_op = mp.iter().any(|(k, _)| k.as_str().map(|s| s.starts_with('$')).unwrap_or(false));
                if has_op {
                    AnyArg::Matcher(parse_matcher(mp)?)
                } else {
                    AnyArg::Value(parse_value(v)?)
                }
            } else {
                AnyArg::Value(parse_value(v)?)
            };
            Ok(Matcher::Any(Box::new(arg)))
        }
        "$every" => {
            let arg = if let YValue::Mapping(mp) = v {
                let has_op = mp.iter().any(|(k, _)| k.as_str().map(|s| s.starts_with('$')).unwrap_or(false));
                if has_op {
                    AnyArg::Matcher(parse_matcher(mp)?)
                } else {
                    AnyArg::Value(parse_value(v)?)
                }
            } else {
                AnyArg::Value(parse_value(v)?)
            };
            Ok(Matcher::Every(Box::new(arg)))
        }
        "$not" => {
            let inner = if let YValue::Mapping(mp) = v {
                let has_op = mp.iter().any(|(k, _)| k.as_str().map(|s| s.starts_with('$')).unwrap_or(false));
                if has_op { parse_matcher(mp)? } else { Matcher::Eq(parse_value(v)?) }
            } else {
                Matcher::Eq(parse_value(v)?)
            };
            Ok(Matcher::Not(Box::new(inner)))
        }
        "$gt" => Ok(Matcher::Gt(parse_value(v)?)),
        "$gte" => Ok(Matcher::Gte(parse_value(v)?)),
        "$lt" => Ok(Matcher::Lt(parse_value(v)?)),
        "$lte" => Ok(Matcher::Lte(parse_value(v)?)),
        "$type" => {
            let s = v
                .as_str()
                .ok_or_else(|| ParseError::Shape("$type expects a string".into()))?
                .to_string();
            Ok(Matcher::Type(s))
        }
        "$matches" => {
            let s = v
                .as_str()
                .ok_or_else(|| ParseError::Shape("$matches expects a string".into()))?
                .to_string();
            Ok(Matcher::Matches(s))
        }
        other => Err(ParseError::Shape(format!("unknown matcher operator: {other}"))),
    }
}

#[allow(dead_code)]
pub fn stringify_value(v: &YValue) -> String {
    match v {
        YValue::String(s) => s.clone(),
        YValue::Number(n) => n.to_string(),
        YValue::Bool(b) => b.to_string(),
        YValue::Null => "null".into(),
        YValue::Sequence(s) => {
            let parts: Vec<String> = s.iter().map(stringify_value).collect();
            format!("[{}]", parts.join(","))
        }
        YValue::Mapping(_) => "<map>".into(),
        YValue::Tagged(t) => stringify_value(&t.value),
    }
}

pub fn binding_path_resolved(spec_path: &Path, binding: &str) -> PathBuf {
    let parent = spec_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
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
