use super::model::{
    AnyValue, CanonicalValue, F64Value, RegistryOperation, RegistrySet, ResolvedComponent, TraceDocument, TraceSpan, TypeRef,
};
use super::{ValidationIssue, issue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn validate_linked_model(trace: &TraceDocument, registry: &RegistrySet, issues: &mut Vec<ValidationIssue>) {
    for span in &trace.spans {
        validate_resource_linkage(span, registry, issues);
        if span.name != "conformance.operation" {
            continue;
        }
        let component_id = span.attributes.get("conformance.component.id").and_then(AnyValue::as_string);
        let operation_name = span.attributes.get("conformance.operation.name").and_then(AnyValue::as_string);
        let (Some(component_id), Some(operation_name)) = (component_id, operation_name) else {
            continue;
        };
        let Some((component, operation)) = find_operation(registry, component_id, operation_name) else {
            if registry.components.contains_key(component_id) {
                model_issue(
                    issues,
                    &span.location,
                    format!("unknown operation '{operation_name}' in '{component_id}'"),
                );
            } else {
                model_issue(issues, &span.location, format!("unknown component '{component_id}'"));
            }
            continue;
        };
        validate_operation(span, component, operation, registry, issues);
    }
}

pub(crate) fn find_operation<'a>(
    registry: &'a RegistrySet,
    component_id: &str,
    operation_name: &str,
) -> Option<(&'a ResolvedComponent, &'a RegistryOperation)> {
    let component = registry.components.get(component_id)?;
    let operation = component
        .component
        .operations
        .iter()
        .find(|operation| operation.name == operation_name)?;
    Some((component, operation))
}

fn validate_resource_linkage(span: &TraceSpan, registry: &RegistrySet, issues: &mut Vec<ValidationIssue>) {
    for (key, expected) in [
        ("conformance.registry.id", registry.root_id.as_str()),
        ("conformance.registry.version", registry.root_version.as_str()),
        ("conformance.registry.digest", registry.root_digest.as_str()),
    ] {
        match span.resource_attributes.get(key).and_then(AnyValue::as_string) {
            Some(actual) if actual == expected => {}
            Some(_) => model_issue(issues, &span.location, format!("trace {key} does not match root registry")),
            None => model_issue(issues, &span.location, format!("missing string attribute '{key}'")),
        }
    }
}

fn validate_operation(
    span: &TraceSpan,
    component: &ResolvedComponent,
    operation: &RegistryOperation,
    registry: &RegistrySet,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(inputs) = span.attributes.get("conformance.operation.inputs").and_then(AnyValue::as_kvlist) {
        let declared = operation
            .inputs
            .iter()
            .map(|input| (input.name.as_str(), &input.value_type))
            .collect::<BTreeMap<_, _>>();
        if inputs.keys().map(String::as_str).collect::<BTreeSet<_>>() == declared.keys().copied().collect() {
            for (name, value_type) in declared {
                validate_typed_value(
                    &inputs[name],
                    value_type,
                    component,
                    registry,
                    &format!("{}.inputs.{name}", span.location),
                    issues,
                );
            }
        } else {
            model_issue(
                issues,
                &format!("{}.inputs", span.location),
                "input names do not match registry operation",
            );
        }
    }

    let observations = operation
        .observations
        .iter()
        .map(|observation| (observation.name.as_str(), &observation.value_type))
        .collect::<BTreeMap<_, _>>();
    for (event_index, event) in span.events.iter().enumerate() {
        if event.name != "conformance.observation" {
            continue;
        }
        let location = format!("{}.events[{event_index}]", span.location);
        let name = event.attributes.get("conformance.observation.name").and_then(AnyValue::as_string);
        let Some(name) = name else {
            continue;
        };
        let Some(value_type) = observations.get(name) else {
            model_issue(issues, &location, format!("observation '{name}' is not declared"));
            continue;
        };
        if let Some(value) = event.attributes.get("conformance.observation.value") {
            validate_typed_value(value, value_type, component, registry, &format!("{location}.value"), issues);
        }
    }

    if span.events.iter().any(|event| event.name == "conformance.fault") {
        return;
    }
    let terminal = span.events.iter().find(|event| {
        matches!(
            event.name.as_str(),
            "conformance.result" | "conformance.empty" | "conformance.error"
        )
    });
    match terminal.map(|event| event.name.as_str()) {
        None => {
            if operation.outcomes.result.is_some() || operation.outcomes.empty {
                model_issue(issues, &span.location, "unit completion not permitted by registry outcomes");
            }
        }
        Some("conformance.result") => {
            let Some(value_type) = operation.outcomes.result.as_ref() else {
                model_issue(issues, &span.location, "result outcome not declared");
                return;
            };
            if let Some(value) = terminal.and_then(|event| event.attributes.get("conformance.result.value")) {
                validate_typed_value(value, value_type, component, registry, &format!("{}.result", span.location), issues);
            }
        }
        Some("conformance.empty") => {
            if !operation.outcomes.empty {
                model_issue(issues, &span.location, "empty outcome not declared");
            }
        }
        Some("conformance.error") => {
            let event = terminal.expect("terminal event exists");
            let name = event.attributes.get("conformance.error.name").and_then(AnyValue::as_string);
            let declaration = name.and_then(|name| operation.outcomes.errors.iter().find(|error| error.name == name));
            let Some(declaration) = declaration else {
                model_issue(issues, &span.location, format!("error outcome {name:?} not declared"));
                return;
            };
            match (declaration.value_type.as_ref(), event.attributes.get("conformance.error.value")) {
                (Some(value_type), Some(value)) => {
                    validate_typed_value(value, value_type, component, registry, &format!("{}.error", span.location), issues);
                }
                (Some(_), None) => model_issue(
                    issues,
                    &span.location,
                    format!("error outcome '{}' requires a value", declaration.name),
                ),
                (None, Some(_)) => model_issue(
                    issues,
                    &span.location,
                    format!("error outcome '{}' does not declare a value", declaration.name),
                ),
                (None, None) => {}
            }
        }
        Some(_) => {}
    }
}

fn validate_typed_value(
    value: &AnyValue,
    value_type: &TypeRef,
    component: &ResolvedComponent,
    registry: &RegistrySet,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match value_type {
        TypeRef::Primitive { name } => validate_primitive(value, name, location, issues),
        TypeRef::Named {
            name,
            component_id,
            registry_id,
        } => {
            let component_id = component_id.as_deref().unwrap_or(&component.component.id);
            let Some(target) = registry.components.get(component_id) else {
                model_issue(issues, location, format!("unknown component '{component_id}'"));
                return;
            };
            if registry_id.as_ref().is_some_and(|registry_id| registry_id != &target.registry_id) {
                model_issue(
                    issues,
                    location,
                    format!("component '{component_id}' does not belong to registry {registry_id:?}"),
                );
                return;
            }
            let Some(definition) = target.component.types.iter().find(|item| item.name() == name) else {
                model_issue(issues, location, format!("unknown named type '{name}'"));
                return;
            };
            validate_typed_value(value, &definition.as_type(), target, registry, location, issues);
        }
        TypeRef::List { items } | TypeRef::Set { items } => {
            let kind = if matches!(value_type, TypeRef::List { .. }) {
                "list"
            } else {
                "set"
            };
            let AnyValue::Array(values) = value else {
                model_issue(issues, location, format!("{kind} must use arrayValue"));
                return;
            };
            let before = issues.len();
            for (index, value) in values.iter().enumerate() {
                validate_typed_value(value, items, component, registry, &format!("{location}[{index}]"), issues);
            }
            if kind == "set" && issues.len() == before {
                let canonical = values
                    .iter()
                    .filter_map(|value| canonical_typed_value(value, items, component, registry))
                    .collect::<Vec<_>>();
                if canonical.iter().collect::<BTreeSet<_>>().len() != canonical.len() {
                    model_issue(issues, location, "set contains duplicate elements");
                }
            }
        }
        TypeRef::Map { keys, values } => {
            if matches!(keys.as_ref(), TypeRef::Primitive { name } if name == "string") {
                let AnyValue::KvList(entries) = value else {
                    model_issue(issues, location, "string-keyed map must use kvlistValue");
                    return;
                };
                for (key, value) in entries {
                    validate_typed_value(value, values, component, registry, &format!("{location}.{key}"), issues);
                }
            } else {
                let AnyValue::Array(entries) = value else {
                    model_issue(issues, location, "non-string-keyed map must use arrayValue");
                    return;
                };
                let mut seen = BTreeSet::new();
                for (index, entry) in entries.iter().enumerate() {
                    let entry_location = format!("{location}[{index}]");
                    let AnyValue::KvList(entry) = entry else {
                        model_issue(issues, &entry_location, "map entry must contain key and value");
                        continue;
                    };
                    if entry.len() != 2 || !entry.contains_key("key") || !entry.contains_key("value") {
                        model_issue(issues, &entry_location, "map entry must contain key and value");
                        continue;
                    }
                    validate_typed_value(&entry["key"], keys, component, registry, &format!("{entry_location}.key"), issues);
                    validate_typed_value(
                        &entry["value"],
                        values,
                        component,
                        registry,
                        &format!("{entry_location}.value"),
                        issues,
                    );
                    if let Some(key) = canonical_typed_value(&entry["key"], keys, component, registry)
                        && !seen.insert(key)
                    {
                        model_issue(issues, location, "map contains duplicate keys");
                    }
                }
            }
        }
        TypeRef::Tuple { items } => {
            let AnyValue::Array(values) = value else {
                model_issue(issues, location, "tuple must use arrayValue");
                return;
            };
            if values.len() != items.len() {
                model_issue(issues, location, "tuple arity does not match registry type");
                return;
            }
            for (index, (value, value_type)) in values.iter().zip(items).enumerate() {
                validate_typed_value(value, value_type, component, registry, &format!("{location}[{index}]"), issues);
            }
        }
        TypeRef::Record { fields } => {
            let AnyValue::KvList(entries) = value else {
                model_issue(issues, location, "record must use kvlistValue");
                return;
            };
            let expected = fields.iter().map(|field| field.name.as_str()).collect::<BTreeSet<_>>();
            if entries.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
                model_issue(issues, location, "record fields do not match registry type");
                return;
            }
            for field in fields {
                validate_typed_value(
                    &entries[&field.name],
                    &field.value_type,
                    component,
                    registry,
                    &format!("{location}.{}", field.name),
                    issues,
                );
            }
        }
        TypeRef::Optional { value: inner } => {
            let AnyValue::KvList(entries) = value else {
                model_issue(issues, location, "optional must use kvlistValue");
                return;
            };
            if entries.len() != 1 {
                model_issue(issues, location, "optional must contain exactly one variant");
                return;
            }
            let (name, payload) = entries.first_key_value().expect("one entry");
            if name == "None" {
                validate_primitive(payload, "unit", &format!("{location}.None"), issues);
            } else if name == "Some" {
                validate_typed_value(payload, inner, component, registry, &format!("{location}.Some"), issues);
            } else {
                model_issue(issues, location, format!("unknown optional variant '{name}'"));
            }
        }
        TypeRef::TaggedUnion { variants } => {
            let AnyValue::KvList(entries) = value else {
                model_issue(issues, location, "tagged union must use kvlistValue");
                return;
            };
            if entries.len() != 1 {
                model_issue(issues, location, "tagged union must contain exactly one variant");
                return;
            }
            let (name, payload) = entries.first_key_value().expect("one entry");
            let Some(variant) = variants.iter().find(|variant| variant.name == *name) else {
                model_issue(issues, location, format!("unknown tagged-union variant '{name}'"));
                return;
            };
            if let Some(value_type) = &variant.payload {
                validate_typed_value(payload, value_type, component, registry, &format!("{location}.{name}"), issues);
            } else {
                validate_primitive(payload, "unit", &format!("{location}.{name}"), issues);
            }
        }
    }
}

fn validate_primitive(value: &AnyValue, primitive: &str, location: &str, issues: &mut Vec<ValidationIssue>) {
    let expected = match primitive {
        "unit" => "kvlistValue",
        "string" | "u64" => "stringValue",
        "bool" => "boolValue",
        "i32" | "i64" | "u32" => "intValue",
        "f32" | "f64" => "doubleValue",
        "bytes" => "bytesValue",
        _ => {
            model_issue(issues, location, format!("unsupported primitive '{primitive}'"));
            return;
        }
    };
    let wire_matches = matches!(
        (expected, value),
        ("kvlistValue", AnyValue::KvList(_))
            | ("stringValue", AnyValue::String(_))
            | ("boolValue", AnyValue::Bool(_))
            | ("intValue", AnyValue::Int(_))
            | ("doubleValue", AnyValue::Double(_))
            | ("bytesValue", AnyValue::Bytes(_))
    );
    if !wire_matches {
        model_issue(issues, location, format!("{primitive} must use {expected}"));
        return;
    }
    match (primitive, value) {
        ("unit", AnyValue::KvList(values)) if !values.is_empty() => {
            model_issue(issues, location, "unit must be an empty kvlistValue");
        }
        ("i32", AnyValue::Int(value)) if i32::try_from(*value).is_err() => {
            model_issue(issues, location, "value is outside i32 range");
        }
        ("u32", AnyValue::Int(value)) if u32::try_from(*value).is_err() => {
            model_issue(issues, location, "value is outside u32 range");
        }
        ("u64", AnyValue::String(value)) => {
            let canonical = value == "0" || (!value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit()));
            if !canonical {
                model_issue(issues, location, "u64 must be a canonical decimal string");
            } else if value.parse::<u64>().is_err() {
                model_issue(issues, location, "value is outside u64 range");
            }
        }
        ("f32", AnyValue::Double(F64Value::Finite(bits))) => {
            let number = f64::from_bits(*bits);
            #[allow(clippy::cast_possible_truncation)]
            let narrowed = number as f32;
            if f64::from(narrowed).to_bits() != number.to_bits() {
                model_issue(issues, location, "value is not exactly representable as f32");
            }
        }
        _ => {}
    }
}

pub(crate) fn canonical_typed_value(
    value: &AnyValue,
    value_type: &TypeRef,
    component: &ResolvedComponent,
    registry: &RegistrySet,
) -> Option<CanonicalValue> {
    match value_type {
        TypeRef::Primitive { name } => canonical_primitive(value, name),
        TypeRef::Named {
            name,
            component_id,
            registry_id,
        } => {
            let component_id = component_id.as_deref().unwrap_or(&component.component.id);
            let target = registry.components.get(component_id)?;
            if registry_id.as_ref().is_some_and(|registry_id| registry_id != &target.registry_id) {
                return None;
            }
            let definition = target.component.types.iter().find(|item| item.name() == name)?;
            canonical_typed_value(value, &definition.as_type(), target, registry)
        }
        TypeRef::List { items } => {
            let AnyValue::Array(values) = value else {
                return None;
            };
            values
                .iter()
                .map(|value| canonical_typed_value(value, items, component, registry))
                .collect::<Option<Vec<_>>>()
                .map(CanonicalValue::List)
        }
        TypeRef::Set { items } => {
            let AnyValue::Array(values) = value else {
                return None;
            };
            values
                .iter()
                .map(|value| canonical_typed_value(value, items, component, registry))
                .collect::<Option<BTreeSet<_>>>()
                .map(CanonicalValue::Set)
        }
        TypeRef::Map { keys, values } => {
            if matches!(keys.as_ref(), TypeRef::Primitive { name } if name == "string") {
                let AnyValue::KvList(entries) = value else {
                    return None;
                };
                entries
                    .iter()
                    .map(|(key, value)| {
                        canonical_typed_value(value, values, component, registry).map(|value| (CanonicalValue::String(key.clone()), value))
                    })
                    .collect::<Option<BTreeMap<_, _>>>()
                    .map(CanonicalValue::Map)
            } else {
                let AnyValue::Array(entries) = value else {
                    return None;
                };
                entries
                    .iter()
                    .map(|entry| {
                        let AnyValue::KvList(entry) = entry else {
                            return None;
                        };
                        Some((
                            canonical_typed_value(entry.get("key")?, keys, component, registry)?,
                            canonical_typed_value(entry.get("value")?, values, component, registry)?,
                        ))
                    })
                    .collect::<Option<BTreeMap<_, _>>>()
                    .map(CanonicalValue::Map)
            }
        }
        TypeRef::Tuple { items } => {
            let AnyValue::Array(values) = value else {
                return None;
            };
            if values.len() != items.len() {
                return None;
            }
            values
                .iter()
                .zip(items)
                .map(|(value, value_type)| canonical_typed_value(value, value_type, component, registry))
                .collect::<Option<Vec<_>>>()
                .map(CanonicalValue::Tuple)
        }
        TypeRef::Record { fields } => {
            let AnyValue::KvList(entries) = value else {
                return None;
            };
            fields
                .iter()
                .map(|field| {
                    canonical_typed_value(entries.get(&field.name)?, &field.value_type, component, registry)
                        .map(|value| (field.name.clone(), value))
                })
                .collect::<Option<BTreeMap<_, _>>>()
                .map(CanonicalValue::Record)
        }
        TypeRef::Optional { value: inner } => {
            let AnyValue::KvList(entries) = value else {
                return None;
            };
            let (name, payload) = entries.first_key_value()?;
            let value = if name == "None" {
                canonical_primitive(payload, "unit")?
            } else if name == "Some" {
                canonical_typed_value(payload, inner, component, registry)?
            } else {
                return None;
            };
            Some(CanonicalValue::Variant(name.clone(), Box::new(value)))
        }
        TypeRef::TaggedUnion { variants } => {
            let AnyValue::KvList(entries) = value else {
                return None;
            };
            let (name, payload) = entries.first_key_value()?;
            let variant = variants.iter().find(|variant| variant.name == *name)?;
            let value = match &variant.payload {
                Some(value_type) => canonical_typed_value(payload, value_type, component, registry)?,
                None => canonical_primitive(payload, "unit")?,
            };
            Some(CanonicalValue::Variant(name.clone(), Box::new(value)))
        }
    }
}

fn canonical_primitive(value: &AnyValue, primitive: &str) -> Option<CanonicalValue> {
    match (primitive, value) {
        ("unit", AnyValue::KvList(values)) if values.is_empty() => Some(CanonicalValue::Unit),
        ("string", AnyValue::String(value)) => Some(CanonicalValue::String(value.clone())),
        ("bool", AnyValue::Bool(value)) => Some(CanonicalValue::Bool(*value)),
        ("i32" | "i64" | "u32", AnyValue::Int(value)) => Some(CanonicalValue::Integer(i128::from(*value))),
        ("u64", AnyValue::String(value)) => value.parse::<u64>().ok().map(|value| CanonicalValue::Integer(i128::from(value))),
        ("f32" | "f64", AnyValue::Double(value)) => Some(CanonicalValue::Float(*value)),
        ("bytes", AnyValue::Bytes(value)) => Some(CanonicalValue::Bytes(value.clone())),
        _ => None,
    }
}

fn model_issue(issues: &mut Vec<ValidationIssue>, location: &str, message: impl Into<String>) {
    issue(issues, location.to_string(), message);
}
