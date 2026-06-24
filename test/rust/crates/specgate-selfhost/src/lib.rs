//! Self-hosting entry point: exposes the harness's own `run_spec` as a spec
//! operation named `"run_spec"`, returning a `SpecEvent`-deriving outcome so the
//! harness can validate ITS OWN spec (`specs/specgate.harness.spec.yaml`)
//! through its own pipeline.
//!
//! The harness's real `RunOutcome` / `CaseResult` / `TraceEvent` types do not
//! derive `SpecEvent`, so they are mirrored here with local `SpecEvent` types
//! whose `to_spec_value` yields the structured `$result` the harness spec
//! asserts: `{ Complete: { results: [ { name, status, traces } ] } }` /
//! `{ Error: { reason } }`.

use specgate::{SpecEvent, ToSpecValue, Value, spec_operation};

/// Identity adapter: the runtime `Value` is already a spec value but does not
/// impl `ToSpecValue` (which every `#[derive(SpecEvent)]` field needs).
pub struct SpecVal(pub Value);

impl std::fmt::Debug for SpecVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ToSpecValue for SpecVal {
    fn to_spec_value(&self) -> Value {
        self.0.clone()
    }
}

#[derive(Debug, SpecEvent)]
pub enum SelfHostTrace {
    Run { operation: String },
    Event { name: String, value: SpecVal },
}

#[derive(Debug, SpecEvent)]
pub struct SelfHostCaseResult {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub status: String,
    #[spec_event]
    pub level: String,
    #[spec_event]
    pub source: SpecVal,
    #[spec_event]
    pub expected: Vec<SpecVal>,
    #[spec_event]
    pub traces: Vec<SelfHostTrace>,
}

#[derive(Debug, SpecEvent)]
pub enum SelfHostOutcome {
    Complete { results: Vec<SelfHostCaseResult> },
    Error { reason: String },
}

fn convert_trace(t: specgate_harness::TraceEvent) -> SelfHostTrace {
    match t {
        specgate_harness::TraceEvent::Run { operation } => SelfHostTrace::Run { operation },
        specgate_harness::TraceEvent::Event { name, value } => SelfHostTrace::Event {
            name,
            value: SpecVal(value),
        },
    }
}

// --- Assertion -> Value serialization -------------------------------------
//
// Renders each result's parsed `expected` assertions back into the documented
// spec form (single-key maps like `{$result: "5"}` and operator maps like
// `{$size: 3}`) so the harness spec can assert them as part of `$result`.

fn assertion_to_value(a: &specgate_harness::Assertion) -> Value {
    use specgate_harness::Assertion as A;
    let mut m = std::collections::BTreeMap::new();
    match a {
        A::Event { name, value } => {
            m.insert(name.clone(), assert_value_to_value(value));
        }
        A::Run { operation } => {
            m.insert("$run".to_string(), Value::String(operation.clone()));
        }
        A::Unordered { items } => {
            m.insert(
                "$unordered".to_string(),
                Value::List(items.iter().map(assertion_to_value).collect()),
            );
        }
        A::Anywhere { items } => {
            m.insert(
                "$anywhere".to_string(),
                Value::List(items.iter().map(assertion_to_value).collect()),
            );
        }
    }
    Value::Map(m)
}

fn assert_value_to_value(v: &specgate_harness::AssertValue) -> Value {
    use specgate_harness::AssertValue as AV;
    match v {
        AV::Exact(val) => val.clone(),
        AV::Matcher(m) => matcher_to_value(m),
    }
}

fn one(key: &str, v: Value) -> Value {
    let mut m = std::collections::BTreeMap::new();
    m.insert(key.to_string(), v);
    Value::Map(m)
}

fn any_arg_to_value(a: &specgate_harness::AnyArg) -> Value {
    use specgate_harness::AnyArg as AA;
    match a {
        AA::Value(v) => v.clone(),
        AA::Matcher(m) => matcher_to_value(m),
    }
}

fn matcher_to_value(m: &specgate_harness::Matcher) -> Value {
    use specgate_harness::Matcher as M;
    match m {
        M::Eq(v) => one("$eq", v.clone()),
        M::Ne(v) => one("$ne", v.clone()),
        M::Size(n) => one("$size", Value::Integer(*n as i64)),
        M::Contains(arg) => one("$contains", any_arg_to_value(arg)),
        M::ContainsAll(items) => one("$containsAll", Value::List(items.clone())),
        M::Excludes(items) => one("$excludes", Value::List(items.clone())),
        M::Match(fields) => one(
            "$match",
            Value::Map(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), assert_value_to_value(v)))
                    .collect(),
            ),
        ),
        M::Exists(b) => one("$exists", Value::Bool(*b)),
        M::Any(arg) => one("$any", any_arg_to_value(arg)),
        M::Every(arg) => one("$every", any_arg_to_value(arg)),
        M::Type(t) => one("$type", Value::String(t.clone())),
        M::Matches(re) => one("$matches", Value::String(re.clone())),
        M::Not(inner) => one("$not", matcher_to_value(inner)),
        M::Gt(v) => one("$gt", v.clone()),
        M::Gte(v) => one("$gte", v.clone()),
        M::Lt(v) => one("$lt", v.clone()),
        M::Lte(v) => one("$lte", v.clone()),
        // Composite: several operators in one mapping — merge into a single map
        // (matching the documented `{ $op1: .., $op2: .. }` form).
        M::Composite(parts) => {
            let mut merged = std::collections::BTreeMap::new();
            for p in parts {
                if let Value::Map(pm) = matcher_to_value(p) {
                    merged.extend(pm);
                }
            }
            Value::Map(merged)
        }
    }
}

fn source_to_value(s: &Option<specgate_harness::Source>) -> Value {
    let mut m = std::collections::BTreeMap::new();
    if let Some(src) = s {
        m.insert(
            "assertion_ids".to_string(),
            Value::List(
                src.assertion_ids
                    .iter()
                    .map(|a| Value::String(a.clone()))
                    .collect(),
            ),
        );
        m.insert("spec".to_string(), Value::String(src.spec.clone()));
        m.insert("section".to_string(), Value::String(src.section.clone()));
    }
    Value::Map(m)
}

// --- run_spec wrapper ------------------------------------------------------

#[spec_operation("run_spec")]
pub fn run_spec(#[spec_input("spec")] spec_path: &str) -> SelfHostOutcome {
    // The harness spec uses repo-root-relative paths, but the generated runner
    // executes with the harness scratch dir
    // (`<repo>/rust/target/specgate-harness/<stem>`) as its `CARGO_MANIFEST_DIR`.
    // Resolve relative paths against the repo root, four levels up.
    let resolved = {
        let p = std::path::Path::new(spec_path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            root.pop();
            root.pop();
            root.pop();
            root.pop();
            root.join(p)
        }
    };
    let resolved = resolved.to_string_lossy().into_owned();
    match specgate_harness::run_spec(&resolved) {
        specgate_harness::RunOutcome::Complete { results } => SelfHostOutcome::Complete {
            results: results
                .into_iter()
                .map(|r| SelfHostCaseResult {
                    name: r.name,
                    status: r.status.as_str().to_string(),
                    level: r.level.as_str().to_string(),
                    source: SpecVal(source_to_value(&r.source)),
                    expected: r
                        .expected
                        .iter()
                        .map(|a| SpecVal(assertion_to_value(a)))
                        .collect(),
                    traces: r.traces.into_iter().map(convert_trace).collect(),
                })
                .collect(),
        },
        specgate_harness::RunOutcome::Error { reason } => SelfHostOutcome::Error { reason },
    }
}
