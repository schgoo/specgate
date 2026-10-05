//! Setup provenance recording and registry-driven input folding.

use super::*;

thread_local! {
    static PENDING: RefCell<BTreeMap<SetupKey, BTreeMap<String, Value>>> =
        const { RefCell::new(BTreeMap::new()) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SetupKey {
    component_id: registry::ComponentName,
    operation_name: registry::OpName,
    module_path: registry::ModulePath,
    fn_name: registry::FnName,
    fills: Option<registry::FieldName>,
}

/// Deferred setup values consumed by annotation-generated operation code.
#[derive(Debug)]
#[doc(hidden)]
pub struct DeferredSetup {
    component_id: registry::ComponentName,
    operation_name: registry::OpName,
    module_path: registry::ModulePath,
    fn_name: registry::FnName,
    fills: Option<registry::FieldName>,
    inputs: Option<Vec<(registry::FieldName, Value)>>,
}

/// Typed identity and provenance for one generated setup declaration.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
#[expect(clippy::exhaustive_structs, reason = "macro setup provenance is a closed generated-code protocol")]
pub struct SetupProvenance {
    /// Owning component.
    pub component_id: registry::ComponentName,
    /// Constructed operation.
    pub operation_name: registry::OpName,
    /// Native module path.
    pub module_path: registry::ModulePath,
    /// Native function name.
    pub fn_name: registry::FnName,
    /// Explicit filled parameter, when selected.
    pub fills: Option<registry::FieldName>,
}

#[doc(hidden)]
#[must_use]
/// Defer setup-input evaluation until generated instrumentation determines
/// that an active capture needs the values.
///
/// # Panics
/// A panic from `inputs` propagates to the caller.
pub fn defer_setup<F>(provenance: SetupProvenance, inputs: F) -> DeferredSetup
where
    F: FnOnce() -> Vec<(registry::FieldName, Value)>,
{
    DeferredSetup {
        component_id: provenance.component_id,
        operation_name: provenance.operation_name,
        module_path: provenance.module_path,
        fn_name: provenance.fn_name,
        fills: provenance.fills,
        inputs: requested().then(inputs),
    }
}

impl DeferredSetup {
    /// Commit setup inputs after the annotated producer returns successfully.
    ///
    /// # Errors
    ///
    /// Returns an error when the setup identity or projected inputs conflict
    /// with values already recorded in the active capture.
    ///
    /// # Panics
    /// Panics when generated setup projection emits the same input name twice.
    pub fn commit(mut self) -> Result<(), CaptureError> {
        let Some(inputs) = self.inputs.take() else {
            return Ok(());
        };
        record_setup(
            SetupProvenance {
                component_id: self.component_id,
                operation_name: self.operation_name,
                module_path: self.module_path,
                fn_name: self.fn_name,
                fills: self.fills,
            },
            || inputs,
        )
    }
}

/// Record one `#[spec_setup]` producer's semantic construction inputs.
///
/// Registry discovery folds a setup's parameters into the public input surface
/// of the operation it constructs, so the next invocation of that exact
/// component + operation adopts these values as its own black-box inputs.
/// Values are projected only while a capture session is active or requested,
/// so ordinary runs pay nothing and observe no behavior change.
///
/// Attribution is by declaration, not by constructed instance: nothing links a
/// returned receiver back to the call that produced it. Running one setup
/// declaration twice in a single capture is therefore accepted only when both
/// runs record value-identical inputs; differing inputs are ambiguous and are
/// rejected instead of silently attributing the last construction to every
/// later invocation.
///
/// # Errors
///
/// Returns an error when one setup declaration runs twice in a capture with
/// differing inputs.
///
/// # Panics
/// Panics when generated setup projection emits the same input name twice.
/// A panic from `inputs` also propagates to the caller.
///
/// # Examples
/// ```
/// use specgate_runtime::capture::{
///     Config, ConfigDeps, SetupProvenance, SpanId, TraceId, finish,
///     record_setup, start,
/// };
/// use specgate_runtime::registry::{ComponentName, FnName, ModulePath, OpName};
///
/// start(Config::builder(ConfigDeps {
///     scenario_name: "setup".into(),
///     trace_id: TraceId::parse("11111111111111111111111111111111")?,
///     run_span_id: SpanId::parse("1111111111111101")?,
///     scenario_span_id: SpanId::parse("1111111111111102")?,
/// }).build()?)?;
/// record_setup(SetupProvenance {
///     component_id: ComponentName::new("example.component"),
///     operation_name: OpName::new("run"),
///     module_path: ModulePath::new("example"),
///     fn_name: FnName::new("setup"),
///     fills: None,
/// }, Vec::new)?;
/// finish()?;
/// # Ok::<(), specgate_runtime::CaptureError>(())
/// ```
pub fn record_setup<F>(provenance: SetupProvenance, inputs: F) -> Result<(), CaptureError>
where
    F: FnOnce() -> Vec<(registry::FieldName, Value)>,
{
    record_values(provenance, inputs)
}

fn record_values<F>(provenance: SetupProvenance, inputs: F) -> Result<(), CaptureError>
where
    F: FnOnce() -> Vec<(registry::FieldName, Value)>,
{
    if !requested() {
        return Ok(());
    }
    let component_id = provenance.component_id;
    let operation_name = provenance.operation_name;
    let module_path = provenance.module_path;
    let fn_name = provenance.fn_name;
    let fills = provenance.fills;
    let mut recorded = BTreeMap::new();
    for (name, value) in inputs() {
        let name = name.as_str();
        assert!(
            recorded.insert(name.to_string(), value).is_none(),
            "generated setup invariant violated: setup '{fn_name}' for '{component_id}::{operation_name}' records input '{name}' twice"
        );
    }
    let key = SetupKey {
        component_id,
        operation_name,
        module_path,
        fn_name,
        fills,
    };
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        match pending.get(&key) {
            Some(existing) if inputs_match(existing, &recorded) => Ok(()),
            Some(existing) => Err(CaptureError::setup_ambiguity(SetupAmbiguity {
                identity: SetupIdentity {
                    component: component_id,
                    operation: operation_name,
                    setup: fn_name,
                },
                earlier: describe_inputs(existing),
                later: describe_inputs(&recorded),
            })),
            None => {
                pending.insert(key, recorded);
                Ok(())
            }
        }
    })
}

/// Exact structural identity of two recorded construction input maps.
///
/// Deliberately stricter than [`Value`]'s `PartialEq`, which equates a list
/// with a set and an integer with a float. Repeat construction is accepted only
/// when the two recordings are the same value in the same representation.
pub(super) fn inputs_match(left: &BTreeMap<String, Value>, right: &BTreeMap<String, Value>) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|((left_name, left_value), (right_name, right_value))| left_name == right_name && values_match(left_value, right_value))
}

fn values_match(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Integer(left), Value::Integer(right)) => left == right,
        (Value::Unsigned(left), Value::Unsigned(right)) => left == right,
        (Value::Float(left), Value::Float(right)) => left.to_bits() == right.to_bits(),
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::List(left), Value::List(right)) => {
            left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| values_match(left, right))
        }
        (Value::Set(left), Value::Set(right)) => {
            left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| values_match(left, right))
        }
        (Value::Map(left), Value::Map(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|((left_key, left_value), (right_key, right_value))| {
                        left_key == right_key && values_match(left_value, right_value)
                    })
        }
        _ => false,
    }
}

/// Render one recorded construction input map for an actionable error message.
fn describe_inputs(inputs: &BTreeMap<String, Value>) -> String {
    if inputs.is_empty() {
        return "no inputs".to_string();
    }
    inputs
        .iter()
        .map(|(name, value)| format!("{name}={value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Construction inputs recorded for one exact component + operation key.
///
/// Attribution never crosses a component or operation boundary: only setups
/// registered for this exact operation contribute. Discovery rejects a
/// component whose folded surface carries one input name twice, so a duplicate
/// here is a real metadata fault rather than something to silently merge.
pub(super) fn recorded_inputs(
    component_id: impl AsRef<str>,
    operation_name: impl AsRef<str>,
) -> Result<BTreeMap<String, Value>, CaptureError> {
    let component_id = component_id.as_ref();
    let operation_name = operation_name.as_ref();
    PENDING.with(|pending| {
        let pending = pending.borrow();
        let mut merged: BTreeMap<String, Value> = BTreeMap::new();
        for (key, inputs) in pending
            .iter()
            .filter(|(key, _inputs)| {
                key.component_id.as_ref() == component_id && key.operation_name.as_ref() == operation_name
            })
        {
            for (name, value) in inputs {
                assert!(
                    merged.insert(name.clone(), value.clone()).is_none(),
                    "capture registry invariant violated: setups for '{component_id}::{operation_name}' record input '{name}' twice; setup '{}' collides with an earlier producer",
                    key.fn_name
                );
            }
        }
        Ok(merged)
    })
}

#[derive(Debug)]
enum SetupContribution {
    Receiver,
    Parameter(String),
}

#[derive(Debug)]
struct ExpectedSetup {
    key: SetupKey,
    contribution: SetupContribution,
}

pub(super) fn folded_inputs(
    component_id: impl AsRef<str>,
    operation_name: impl AsRef<str>,
) -> Result<(BTreeMap<String, Value>, BTreeSet<String>), CaptureError> {
    let component_id = component_id.as_ref();
    let operation_name = operation_name.as_ref();
    let Some(operation) = registry::OPERATIONS
        .iter()
        .find(|candidate| !candidate.is_setup && candidate.component == component_id && candidate.name == operation_name)
    else {
        return Ok((recorded_inputs(component_id, operation_name)?, BTreeSet::new()));
    };

    let mut filled = BTreeSet::new();
    let mut required_setups = Vec::with_capacity(registry::OPERATIONS.len());
    let mut receiver_claimed = false;
    for setup in registry::OPERATIONS
        .iter()
        .filter(|candidate| candidate.is_setup && candidate.component == component_id && candidate.name == operation_name)
    {
        let contribution = if setup.fills.is_none() {
            let mut candidates = operation
                .params
                .iter()
                .filter(|field| !filled.contains(field.name()) && types_match(field.rust_type(), setup.return_type))
                .map(|field| field.name());
            match (candidates.next(), candidates.next()) {
                (Some(only), None) => Some(SetupContribution::Parameter(only.to_string())),
                (None, _) if !receiver_claimed => {
                    receiver_claimed = true;
                    Some(SetupContribution::Receiver)
                }
                _ => None,
            }
        } else if let Some(fills) = setup.fills
            && operation.params.iter().any(|field| field.name() == fills)
        {
            Some(SetupContribution::Parameter(fills.to_string()))
        } else {
            None
        };

        if let Some(contribution) = contribution {
            if let SetupContribution::Parameter(name) = &contribution {
                filled.insert(name.clone());
            }
            required_setups.push(ExpectedSetup {
                key: SetupKey {
                    component_id: registry::ComponentName::new(setup.component),
                    operation_name: registry::OpName::new(setup.name),
                    module_path: registry::ModulePath::new(setup.module_path),
                    fn_name: registry::FnName::new(setup.fn_name),
                    fills: setup.fills.map(registry::FieldName::new),
                },
                contribution,
            });
        }
    }

    PENDING.with(|pending| {
        let pending = pending.borrow();
        let mut merged = BTreeMap::new();
        for setup in &required_setups {
            let Some(inputs) = pending.get(&setup.key) else {
                return Err(CaptureError::setup_missing(
                    component_id,
                    operation_name,
                    describe_setup(setup),
                ));
            };
            for (name, value) in inputs {
                assert!(
                    merged.insert(name.clone(), value.clone()).is_none(),
                    "capture registry invariant violated: setups for '{component_id}::{operation_name}' record input '{name}' twice; setup '{}' collides with an earlier producer",
                    setup.key.fn_name
                );
            }
        }
        Ok((merged, filled))
    })
}

pub(super) fn clear() {
    PENDING.with(|pending| pending.borrow_mut().clear());
}

fn describe_setup(setup: &ExpectedSetup) -> String {
    let function = format!("'{}::{}'", setup.key.module_path, setup.key.fn_name);
    match &setup.contribution {
        SetupContribution::Receiver => format!("setup {function} constructing the receiver"),
        SetupContribution::Parameter(name) if setup.key.fills.is_none() => {
            format!("setup {function} inferring folded parameter '{name}'")
        }
        SetupContribution::Parameter(name) => format!("setup {function} filling parameter '{name}'"),
    }
}

/// Compare declared types without allocating temporary normalized strings.
fn types_match(left: impl AsRef<str>, right: impl AsRef<str>) -> bool {
    let left = left.as_ref();
    let right = right.as_ref();
    left.chars()
        .filter(|value| !value.is_whitespace())
        .eq(right.chars().filter(|value| !value.is_whitespace()))
}
