//! OTLP span linkage validation and replay scenario assembly.

use super::{
    BTreeMap, COMPONENT_ID, CTSC_VERSION, ComponentId, EMPTY_EVENT, ERROR_EVENT, ERROR_NAME, ERROR_VALUE, FAULT_EVENT, Input, OBS_EVENT,
    OBS_NAME, OPERATION_INPUTS, OPERATION_NAME, OPERATION_SPAN, Operation, OperationName, PARALLEL_SPAN, REGISTRY_DIGEST, REGISTRY_ID,
    REGISTRY_VERSION, RESULT_EVENT, RESULT_VALUE, RUN_SPAN, Registry, SCENARIO_INDEX, SCENARIO_NAME, SCENARIO_SPAN, SPAN_WIDTH, Scenario,
    ScenarioIndex, ScenarioName, TARGET_LANGUAGE, TARGET_NAME, TOOL_NAME, TOOL_VERSION_KEY, TRACE_WIDTH, Type, VERSION_KEY,
    ValidatedManifest, Value, error,
};
mod wire;
pub(super) use wire::OtlpDoc as Document;
use wire::{AnyValue, DoubleValue, KeyValue, KeyValueList, Span};

struct SpanRef<'a> {
    span: &'a Span,
    position: usize,
}

struct ScenarioRef<'a> {
    trace_id: &'a str,
    span_id: &'a str,
    name: ScenarioName,
    index: ScenarioIndex,
    position: usize,
}

pub(super) fn decode_scenarios(
    document: &Document,
    manifest: &ValidatedManifest,
    registry: &Registry,
) -> Result<Vec<Scenario>, error::Error> {
    if document.resource_spans.len() != 1 {
        return Err(format!(
            "reference OTLP must contain exactly one resourceSpans entry, found {}",
            document.resource_spans.len()
        )
        .into());
    }
    let resource = &document.resource_spans[0];
    let resource_attributes = attribute_map(&resource.resource.attributes, "reference resource")?;
    require_string(&resource_attributes, VERSION_KEY, Some(CTSC_VERSION), "reference resource")?;
    require_string(&resource_attributes, TOOL_NAME, None, "reference resource")?;
    require_string(&resource_attributes, TOOL_VERSION_KEY, None, "reference resource")?;
    require_string(&resource_attributes, TARGET_NAME, None, "reference resource")?;
    require_string(&resource_attributes, TARGET_LANGUAGE, None, "reference resource")?;
    require_string(
        &resource_attributes,
        REGISTRY_ID,
        Some(registry.identity.id.as_str()),
        "reference resource",
    )?;
    require_string(
        &resource_attributes,
        REGISTRY_VERSION,
        Some(registry.identity.version.as_str()),
        "reference resource",
    )?;
    require_string(
        &resource_attributes,
        REGISTRY_DIGEST,
        Some(registry.identity.digest.as_str()),
        "reference resource",
    )?;

    let spans = resource
        .scope_spans
        .iter()
        .flat_map(|scope| scope.spans.iter())
        .enumerate()
        .map(|(position, span)| SpanRef { span, position })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return Err("reference OTLP contains no spans".to_string().into());
    }

    let mut by_id = BTreeMap::new();
    for span_ref in &spans {
        validate_id(&span_ref.span.trace_id, TRACE_WIDTH, "trace ID")?;
        validate_id(&span_ref.span.span_id, SPAN_WIDTH, "span ID")?;
        if by_id
            .insert((span_ref.span.trace_id.as_ref(), span_ref.span.span_id.as_ref()), span_ref.span)
            .is_some()
        {
            return Err(format!(
                "reference OTLP contains duplicate span ID '{}' in trace '{}'",
                span_ref.span.span_id, span_ref.span.trace_id
            )
            .into());
        }
        parse_time(&span_ref.span.start_time_unix_nano, || format!("span '{}'", span_ref.span.span_id))?;
    }

    let runs = spans
        .iter()
        .filter(|span_ref| span_ref.span.name.as_ref() == RUN_SPAN)
        .collect::<Vec<_>>();
    if runs.len() != 1 || !runs[0].span.parent_span_id.is_empty() {
        return Err(format!(
            "reference OTLP must contain exactly one root conformance.run span, found {}",
            runs.len()
        )
        .into());
    }

    let mut scenario_refs = Vec::with_capacity(spans.len());
    for span_ref in spans.iter().filter(|span_ref| span_ref.span.name.as_ref() == SCENARIO_SPAN) {
        let parent = by_id
            .get(&(span_ref.span.trace_id.as_ref(), span_ref.span.parent_span_id.as_ref()))
            .ok_or_else(|| format!("scenario span '{}' has an unresolved parent", span_ref.span.span_id))?;
        if parent.name.as_ref() != RUN_SPAN {
            return Err(format!("scenario span '{}' parent is not conformance.run", span_ref.span.span_id).into());
        }
        let attributes = attribute_map(&span_ref.span.attributes, format!("scenario span '{}'", span_ref.span.span_id))?;
        let name = require_string(
            &attributes,
            SCENARIO_NAME,
            None,
            format!("scenario span '{}'", span_ref.span.span_id),
        )?
        .to_string();
        let name = ScenarioName::try_new(name)?;
        let index = require_integer(&attributes, SCENARIO_INDEX, format!("scenario span '{}'", span_ref.span.span_id))?;
        let index = ScenarioIndex::new(index).ok_or_else(|| format!("scenario '{name}' has negative index {index}"))?;
        scenario_refs.push(ScenarioRef {
            trace_id: &span_ref.span.trace_id,
            span_id: &span_ref.span.span_id,
            name,
            index,
            position: span_ref.position,
        });
    }
    scenario_refs.sort_by_key(|scenario| (scenario.index, scenario.position));
    for (expected, scenario) in scenario_refs.iter().enumerate() {
        let expected = i64::try_from(expected).map_err(|_error| "reference scenario index exceeds i64".to_string())?;
        if scenario.index.get() != expected {
            return Err(format!(
                "reference scenario indexes must be unique and contiguous from zero; expected {expected}, found {}",
                scenario.index.get()
            )
            .into());
        }
    }
    let trace_names = scenario_refs.iter().map(|scenario| scenario.name.as_str()).collect::<Vec<_>>();
    let manifest_names = manifest.scenario_names.iter().map(String::as_str).collect::<Vec<_>>();
    if trace_names != manifest_names {
        return Err(format!("reference scenario order/names {trace_names:?} do not match capture manifest {manifest_names:?}").into());
    }

    let mut decoded_operations = BTreeMap::new();
    for span_ref in spans.iter().filter(|span_ref| span_ref.span.name.as_ref() == OPERATION_SPAN) {
        let parent = by_id
            .get(&(span_ref.span.trace_id.as_ref(), span_ref.span.parent_span_id.as_ref()))
            .ok_or_else(|| format!("operation span '{}' has an unresolved parent", span_ref.span.span_id))?;
        if parent.name.as_ref() != SCENARIO_SPAN && parent.name.as_ref() != OPERATION_SPAN {
            return Err(format!(
                "operation span '{}' parent '{}' is not a scenario or operation",
                span_ref.span.span_id, parent.name
            )
            .into());
        }
        decoded_operations.insert(
            (span_ref.span.trace_id.as_ref(), span_ref.span.span_id.as_ref()),
            decode_operation(span_ref.span, registry)?,
        );
    }
    if spans.iter().any(|span_ref| span_ref.span.name.as_ref() == PARALLEL_SPAN) {
        return Err("reference OTLP parallel spans are unsupported by the first replay slice"
            .to_string()
            .into());
    }
    let mut scenarios = Vec::with_capacity(scenario_refs.len());
    let mut top_level = Vec::with_capacity(spans.len());
    for scenario in scenario_refs {
        top_level.clear();
        top_level.extend(spans.iter().filter(|span_ref| {
            span_ref.span.name.as_ref() == OPERATION_SPAN
                && span_ref.span.trace_id.as_ref() == scenario.trace_id
                && span_ref.span.parent_span_id.as_ref() == scenario.span_id
        }));
        top_level.sort_by_key(|span_ref| {
            (
                parse_time(&span_ref.span.start_time_unix_nano, || "top-level operation".to_string())
                    .expect("top-level operation timestamps were validated before sorting"),
                span_ref.position,
            )
        });
        if top_level.is_empty() {
            return Err(format!("reference scenario '{}' contains no top-level operations", scenario.name).into());
        }
        let operations = top_level
            .iter()
            .map(|span_ref| {
                decoded_operations
                    .get(&(span_ref.span.trace_id.as_ref(), span_ref.span.span_id.as_ref()))
                    .cloned()
                    .expect("every validated operation span was decoded before scenario assembly")
            })
            .collect();
        scenarios.push(Scenario {
            name: scenario.name,
            index: scenario.index,
            operations,
        });
    }
    Ok(scenarios)
}

fn decode_operation(span: &Span, registry: &Registry) -> Result<Operation, error::Error> {
    let location = format!("operation span '{}'", span.span_id);
    let attributes = attribute_map(&span.attributes, &location)?;
    let component_id = ComponentId::try_new(require_string(&attributes, COMPONENT_ID, None, &location)?)?;
    let operation_name = OperationName::try_new(require_string(&attributes, OPERATION_NAME, None, &location)?)?;
    let declaration = registry
        .operations
        .iter()
        .find(|operation| component_id == operation.component_id().as_str() && operation_name == operation.name().as_str())
        .ok_or_else(|| format!("reference {location} names unknown operation '{component_id}::{operation_name}'"))?;
    let input_value = attributes
        .get(OPERATION_INPUTS)
        .ok_or_else(|| format!("reference {location} is missing conformance.operation.inputs"))?;
    let AnyValue::Kvlist(input_list) = input_value else {
        return Err(format!("reference {location} operation inputs must use kvlistValue").into());
    };
    let input_values = kvlist_map(input_list, format!("{location} inputs"))?;
    if input_values.len() != declaration.inputs().len() {
        return Err(format!(
            "reference {location} input names do not match registry operation '{}::{}'",
            declaration.component_id(),
            declaration.name()
        )
        .into());
    }
    let mut inputs = Vec::with_capacity(declaration.inputs().len());
    for input in declaration.inputs() {
        let value = input_values.get(input.name.as_str()).ok_or_else(|| {
            format!(
                "reference {location} is missing declared input '{}' for '{}::{}'",
                input.name,
                declaration.component_id(),
                declaration.name()
            )
        })?;
        inputs.push(Input {
            name: input.name.clone(),
            value_type: input.value_type.clone(),
            value: decode_value(value, &input.value_type, format!("{location} input '{}'", input.name))?,
        });
    }
    for actual in input_values.keys() {
        if !declaration.inputs().iter().any(|input| input.name == *actual) {
            return Err(format!(
                "reference {location} contains undeclared input '{actual}' for '{}::{}'",
                declaration.component_id(),
                declaration.name()
            )
            .into());
        }
    }

    let mut result_count = 0_usize;
    let mut empty_count = 0_usize;
    let mut error_count = 0_usize;
    let mut has_fault = false;
    for event in &span.events {
        match event.name.as_ref() {
            RESULT_EVENT => {
                result_count += 1;
                let output = declaration.output().ok_or_else(|| {
                    format!(
                        "reference {location} emits a result but registry operation '{}::{}' declares no result",
                        declaration.component_id(),
                        declaration.name()
                    )
                })?;
                let event_attributes = attribute_map(&event.attributes, format!("{location} result"))?;
                let value = event_attributes
                    .get(RESULT_VALUE)
                    .ok_or_else(|| format!("reference {location} result is missing conformance.result.value"))?;
                let _ = decode_value(value, output, format!("{location} result"))?;
            }
            OBS_EVENT => {
                let event_attributes = attribute_map(&event.attributes, format!("{location} observation"))?;
                let name = require_string(&event_attributes, OBS_NAME, None, format!("{location} observation"))?;
                return Err(format!("reference {location} contains unsupported observation '{name}' in the first replay slice").into());
            }
            FAULT_EVENT => has_fault = true,
            EMPTY_EVENT => {
                empty_count += 1;
                if !declaration.empty() {
                    return Err(format!(
                        "reference {location} emits empty but registry operation '{}::{}' does not declare it",
                        declaration.component_id(),
                        declaration.name()
                    )
                    .into());
                }
            }
            ERROR_EVENT => {
                error_count += 1;
                let attributes = attribute_map(&event.attributes, format!("{location} error"))?;
                let name = require_string(&attributes, ERROR_NAME, None, format!("{location} error"))?;
                let declared = declaration.errors().iter().find(|error| error.name == name).ok_or_else(|| {
                    format!(
                        "reference {location} emits undeclared error '{name}' for '{}::{}'",
                        declaration.component_id(),
                        declaration.name()
                    )
                })?;
                match (&declared.value_type, attributes.get(ERROR_VALUE)) {
                    (Some(value_type), Some(value)) => {
                        let _ = decode_value(value, value_type, format!("{location} error '{name}'"))?;
                    }
                    (Some(_), None) => return Err(format!("reference {location} error '{name}' is missing its value").into()),
                    (None, Some(_)) => return Err(format!("reference {location} error '{name}' declares no value").into()),
                    (None, None) => {}
                }
            }
            other => return Err(format!("reference {location} contains unsupported CTSC event '{other}'").into()),
        }
    }
    let terminal_count = result_count + empty_count + error_count + usize::from(has_fault);
    if terminal_count > 1 {
        return Err(format!("reference {location} contains multiple terminal events").into());
    }
    if declaration.output().is_some() && terminal_count == 0 {
        return Err(format!(
            "reference {location} has no result or fault for result-bearing operation '{}::{}'",
            declaration.component_id(),
            declaration.name()
        )
        .into());
    }

    Ok(Operation {
        component_id,
        operation_name,
        inputs,
        output: declaration.output().cloned(),
    })
}

fn attribute_map(
    attributes: &(impl AsRef<[KeyValue]> + ?Sized),
    location: impl AsRef<str>,
) -> Result<BTreeMap<&str, &AnyValue>, error::Error> {
    let attributes = attributes.as_ref();
    let location = location.as_ref();
    let mut result = BTreeMap::new();
    for attribute in attributes {
        if result.insert(attribute.key.as_ref(), &attribute.value).is_some() {
            return Err(format!("{location} contains duplicate attribute '{}'", attribute.key).into());
        }
    }
    Ok(result)
}

fn kvlist_map(list: &KeyValueList, location: impl AsRef<str>) -> Result<BTreeMap<&str, &AnyValue>, error::Error> {
    let location = location.as_ref();
    let mut result = BTreeMap::new();
    for entry in &list.values {
        if result.insert(entry.key.as_ref(), &entry.value).is_some() {
            return Err(format!("{location} contains duplicate key '{}'", entry.key).into());
        }
    }
    Ok(result)
}

fn require_string<'a>(
    attributes: &'a BTreeMap<&str, &'a AnyValue>,
    key: impl AsRef<str>,
    expected: Option<&str>,
    location: impl AsRef<str>,
) -> Result<&'a str, error::Error> {
    let location = location.as_ref();
    let key = key.as_ref();
    let value = attributes
        .get(key)
        .ok_or_else(|| format!("{location} is missing string attribute '{key}'"))?;
    let AnyValue::String(value) = value else {
        return Err(format!("{location} attribute '{key}' must use stringValue").into());
    };
    if let Some(expected) = expected
        && value != expected
    {
        return Err(format!("{location} attribute '{key}' is '{value}', expected '{expected}'").into());
    }
    Ok(value)
}

fn require_integer(attributes: &BTreeMap<&str, &AnyValue>, key: impl AsRef<str>, location: impl AsRef<str>) -> Result<i64, error::Error> {
    let location = location.as_ref();
    let key = key.as_ref();
    let value = attributes
        .get(key)
        .ok_or_else(|| format!("{location} is missing integer attribute '{key}'"))?;
    let AnyValue::Int(value) = value else {
        return Err(format!("{location} attribute '{key}' must use intValue").into());
    };
    value
        .parse::<i64>()
        .map_err(|error| error::Error::from(format!("{location} attribute '{key}' is not an i64 decimal integer: {error}")))
}

fn validate_id(value: impl AsRef<str>, length: usize, label: impl AsRef<str>) -> Result<(), error::Error> {
    let label = label.as_ref();
    let value = value.as_ref();
    if value.len() != length
        || value.bytes().all(|byte| byte == b'0')
        || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("reference {label} '{value}' must be {length} lowercase hexadecimal characters and nonzero").into());
    }
    Ok(())
}

fn parse_time(value: impl AsRef<str>, location: impl FnOnce() -> String) -> Result<i64, error::Error> {
    let value = value.as_ref();
    let parsed = match value.parse::<i64>() {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(format!("{} startTimeUnixNano is not an i64 decimal integer: {error}", location()).into());
        }
    };
    if parsed < 0 {
        return Err(format!("{} startTimeUnixNano must be non-negative", location()).into());
    }
    Ok(parsed)
}

mod value;
use value::decode_value;
