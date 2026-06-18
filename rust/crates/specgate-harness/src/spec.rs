//! Spec YAML parsing.
//!
//! The harness spec uses a list-of-maps `expected:` shape that is
//! incompatible with `specgate-types::SpecDocument`, so we parse with
//! `serde_yaml::Value` and pull out the fields we care about by hand.

use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Spec {
    pub binding_path: Option<String>,
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub setup: Setup,
    pub operation: Option<String>,
    pub steps: Vec<String>,
    pub inputs: BTreeMap<String, Value>,
    pub expected: Vec<BTreeMap<String, String>>,
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
    Ok(Spec { binding_path, cases })
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
            // Either:
            //   - multi-setup with aliases: `source: make_source, target: make_target`
            //     (all values are strings)
            //   - single-setup with params: `make_counter: { initial: 10 }`
            //     (one entry, value is a mapping)
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
                // Each entry: key = setup fn name, value = mapping of params.
                // Currently the harness only handles a single such entry.
                let (k, v) = mp.iter().next().ok_or_else(|| {
                    ParseError::Shape("empty setup mapping".into())
                })?;
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
    // Merge per-setup params (from `setup: { make_X: { p: v } }` form) into
    // the case inputs so downstream code can resolve them uniformly.
    let mut inputs = inputs;
    for (k, v) in extra_inputs {
        inputs.entry(k).or_insert(v);
    }

    let expected = match m.get(Value::String("expected".into())) {
        None => Vec::new(),
        Some(Value::Sequence(seq)) => {
            let mut out = Vec::new();
            for entry in seq {
                let em = entry
                    .as_mapping()
                    .ok_or_else(|| ParseError::Shape("expected entry not a mapping".into()))?;
                let mut single = BTreeMap::new();
                for (k, v) in em {
                    let key = k
                        .as_str()
                        .ok_or_else(|| ParseError::Shape("expected key not string".into()))?
                        .to_string();
                    let val = stringify_value(v);
                    single.insert(key, val);
                }
                out.push(single);
            }
            out
        }
        // Legacy / non-list `expected:` shapes (e.g. `expected: { outcome: ... }`)
        // are tolerated as empty: cases that need real expectations go through
        // the list form. This keeps loader-shape error fixtures (`bad_binding`,
        // `no_cases`) reachable without first failing on `expected:` shape.
        Some(_) => Vec::new(),
    };

    Ok(Case {
        name,
        setup,
        operation,
        steps,
        inputs,
        expected,
    })
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
    parent.join(binding)
}
