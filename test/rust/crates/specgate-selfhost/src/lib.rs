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

use specgate::{SpecEvent, Value, spec_operation};

specgate::spec_component!("specgate.harness");

#[derive(Debug, SpecEvent)]
pub enum SelfHostTrace {
    Run { operation: String },
    Event { name: String, value: Value },
}

#[derive(Debug, SpecEvent)]
pub struct SelfHostTargetFailure {
    #[spec_event]
    pub target: String,
    #[spec_event]
    pub traces: Vec<SelfHostTrace>,
    #[spec_event]
    pub mismatch: String,
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
    pub source: Value,
    #[spec_event]
    pub expected: Vec<Value>,
    #[spec_event]
    pub traces: Vec<SelfHostTrace>,
    #[spec_event]
    pub target_failures: Vec<SelfHostTargetFailure>,
}

#[derive(Debug, SpecEvent)]
pub enum SelfHostOutcome {
    Complete { results: Vec<SelfHostCaseResult> },
    Error { reason: String },
}

fn convert_trace(t: specgate_harness::TraceEvent) -> SelfHostTrace {
    match t {
        specgate_harness::TraceEvent::Run { operation } => SelfHostTrace::Run { operation },
        specgate_harness::TraceEvent::Event { name, value } => SelfHostTrace::Event { name, value },
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

fn convert_target_failure(tf: specgate_harness::TargetFailure) -> SelfHostTargetFailure {
    SelfHostTargetFailure {
        target: tf.target,
        traces: tf.traces.into_iter().map(convert_trace).collect(),
        mismatch: tf.mismatch,
    }
}

// --- run_spec wrapper ------------------------------------------------------

#[spec_operation("run_spec", spec = "specgate.harness")]
pub fn run_spec(#[spec_input("spec")] spec_path: &str) -> SelfHostOutcome {
    run_spec_inner(spec_path)
}

#[spec_operation("run_spec", spec = "specgate.conformance")]
pub fn run_conformance_spec(#[spec_input("spec")] spec_path: &str) -> SelfHostOutcome {
    run_spec_inner(spec_path)
}

fn run_spec_inner(spec_path: &str) -> SelfHostOutcome {
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
                    source: source_to_value(&r.source),
                    expected: r.expected.iter().map(assertion_to_value).collect(),
                    traces: r.traces.into_iter().map(convert_trace).collect(),
                    target_failures: r
                        .target_failures
                        .into_iter()
                        .map(convert_target_failure)
                        .collect(),
                })
                .collect(),
        },
        specgate_harness::RunOutcome::Error { reason } => SelfHostOutcome::Error { reason },
    }
}

// --- discover mirror types -------------------------------------------------
//
// The harness's `DiscoveredSchema` family does not derive `SpecEvent`, so it is
// mirrored here with local `SpecEvent` types whose `to_spec_value` yields the
// structured `$result` the conformance spec's `discover_*` cases assert:
// `{ Complete: { schema: {...}, targets: [ { target, outcome: { SelfDescribed:
// { schema } | NotSelfDescribing: { reason } } } ] } }` / `{ Error: { reason } }`.

#[derive(Debug, SpecEvent)]
pub struct DiscInput {
    #[spec_event]
    pub name: String,
    #[spec_event(name = "type")]
    pub ty: String,
}

#[derive(Debug, SpecEvent)]
pub struct DiscOperation {
    #[spec_event]
    pub name: String,
    #[spec_event(name = "async")]
    pub is_async: bool,
    #[spec_event]
    pub inputs: Vec<DiscInput>,
    #[spec_event]
    pub output: String,
}

#[derive(Debug, SpecEvent)]
pub struct DiscField {
    #[spec_event]
    pub name: String,
    #[spec_event(name = "type")]
    pub ty: String,
}

#[derive(Debug, SpecEvent)]
pub struct DiscVariant {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub fields: Vec<DiscField>,
}

#[derive(Debug, SpecEvent)]
pub struct DiscType {
    #[spec_event]
    pub name: String,
    #[spec_event]
    pub kind: String,
    #[spec_event]
    pub fields: Vec<DiscField>,
    #[spec_event]
    pub variants: Vec<DiscVariant>,
}

#[derive(Debug, SpecEvent)]
pub struct DiscSchema {
    #[spec_event]
    pub component: String,
    #[spec_event]
    pub operations: Vec<DiscOperation>,
    #[spec_event]
    pub types: Vec<DiscType>,
}

#[derive(Debug, SpecEvent)]
pub enum DiscTargetOutcome {
    SelfDescribed { schema: DiscSchema },
    NotSelfDescribing { reason: String },
}

#[derive(Debug, SpecEvent)]
pub struct DiscTargetDiscovery {
    #[spec_event]
    pub target: String,
    #[spec_event]
    pub outcome: DiscTargetOutcome,
}

#[derive(Debug, SpecEvent)]
pub enum DiscOutcome {
    Complete {
        schema: DiscSchema,
        targets: Vec<DiscTargetDiscovery>,
    },
    Error {
        reason: String,
    },
}

fn convert_field(f: specgate_harness::DiscoveredField) -> DiscField {
    DiscField { name: f.name, ty: f.ty }
}

fn convert_variant(v: specgate_harness::DiscoveredVariant) -> DiscVariant {
    DiscVariant {
        name: v.name,
        fields: v.fields.into_iter().map(convert_field).collect(),
    }
}

fn convert_type(t: specgate_harness::DiscoveredType) -> DiscType {
    DiscType {
        name: t.name,
        kind: t.kind,
        fields: t.fields.into_iter().map(convert_field).collect(),
        variants: t.variants.into_iter().map(convert_variant).collect(),
    }
}

fn convert_schema(s: specgate_harness::DiscoveredSchema) -> DiscSchema {
    DiscSchema {
        component: s.component,
        operations: s
            .operations
            .into_iter()
            .map(|op| DiscOperation {
                name: op.name,
                is_async: op.is_async,
                inputs: op
                    .inputs
                    .into_iter()
                    .map(|i| DiscInput { name: i.name, ty: i.ty })
                    .collect(),
                output: op.output,
            })
            .collect(),
        types: s.types.into_iter().map(convert_type).collect(),
    }
}

// --- discover wrapper ------------------------------------------------------

#[spec_operation("discover", spec = "specgate.conformance")]
pub fn discover(#[spec_input("spec")] spec_path: &str) -> DiscOutcome {
    discover_inner(spec_path)
}

fn discover_inner(spec_path: &str) -> DiscOutcome {
    // Same repo-root-relative path resolution as `run_spec_inner`: the runner
    // executes with the harness scratch dir as its `CARGO_MANIFEST_DIR`, four
    // levels below the repo root.
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
    match specgate_harness::discover(&resolved) {
        specgate_harness::DiscoverOutcome::Complete { schema, targets } => DiscOutcome::Complete {
            schema: convert_schema(schema),
            targets: targets.into_iter().map(convert_target).collect(),
        },
        specgate_harness::DiscoverOutcome::Error { reason } => DiscOutcome::Error { reason },
    }
}

fn convert_target(t: specgate_harness::TargetDiscovery) -> DiscTargetDiscovery {
    let outcome = match t.outcome {
        specgate_harness::TargetOutcome::SelfDescribed { schema } => DiscTargetOutcome::SelfDescribed {
            schema: convert_schema(schema),
        },
        specgate_harness::TargetOutcome::NotSelfDescribing { reason } => DiscTargetOutcome::NotSelfDescribing { reason },
    };
    DiscTargetDiscovery { target: t.target, outcome }
}
