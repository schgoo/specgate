//! Link validated Trace Core spans to Registry Core declarations.
//!
//! The validator resolves component and operation identity, checks declared inputs,
//! observations, and terminal outcomes, and recursively canonicalizes composite values
//! according to registry types. Diagnostics retain stable artifact locations; malformed,
//! missing, duplicate, undeclared, and non-canonical values remain reportable failures.

use super::model::{
    AnyValue, CanonicalValue, F64Value, RegistryOperation, RegistrySet, ResolvedComponent, TraceDocument, TraceSpan, TypeRef,
};
use super::{ValidationIssue, issue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

// CTSC Trace Core names. Changing these requires coordinated producer and
// validator updates because they are serialized interoperability keys.
const OP_SPAN: &str = "conformance.operation";
const COMPONENT_ATTR: &str = "conformance.component.id";
const OP_NAME_ATTR: &str = "conformance.operation.name";
const OP_INPUTS_ATTR: &str = "conformance.operation.inputs";
const REGISTRY_ATTR: &str = "conformance.registry.id";
const REGISTRY_VERSION_ATTR: &str = "conformance.registry.version";
const REGISTRY_DIGEST_ATTR: &str = "conformance.registry.digest";
const OBS_EVENT: &str = "conformance.observation";
const OBS_NAME_ATTR: &str = "conformance.observation.name";
const OBS_VALUE_ATTR: &str = "conformance.observation.value";
const FAULT: &str = "conformance.fault";
const RESULT: &str = "conformance.result";
const RESULT_ATTR: &str = "conformance.result.value";
const EMPTY: &str = "conformance.empty";
const ERROR: &str = "conformance.error";
const ABANDONED: &str = "conformance.abandoned";
const ERROR_NAME_ATTR: &str = "conformance.error.name";
const ERROR_VALUE_ATTR: &str = "conformance.error.value";
// Registry primitive and optional-variant spellings are normative CTSC schema
// values. Producers and validators must migrate these constants together.
const PRIMITIVE_UNIT: &str = "unit";
const PRIMITIVE_STRING: &str = "string";
const PRIMITIVE_BOOL: &str = "bool";
const PRIMITIVE_I32: &str = "i32";
const PRIMITIVE_I64: &str = "i64";
const PRIMITIVE_U32: &str = "u32";
const PRIMITIVE_U64: &str = "u64";
const PRIMITIVE_F32: &str = "f32";
const PRIMITIVE_F64: &str = "f64";
const PRIMITIVE_BYTES: &str = "bytes";
const OPTION_NONE: &str = "None";
const OPTION_SOME: &str = "Some";

fn index_capacity(location: impl AsRef<str>, suffix: impl AsRef<str>) -> usize {
    location.as_ref().len() + suffix.as_ref().len() + usize::MAX.ilog10() as usize + 1
}

pub(crate) fn check_linked(trace: &TraceDocument, registry: &RegistrySet, issues: &mut Vec<ValidationIssue>) {
    for span in &trace.spans {
        validate_linkage(span, registry, issues);
        if span.name != OP_SPAN {
            continue;
        }
        let component_id = span.attributes.get(COMPONENT_ATTR).and_then(AnyValue::as_string);
        let operation_name = span.attributes.get(OP_NAME_ATTR).and_then(AnyValue::as_string);
        let (Some(component_id), Some(operation_name)) = (component_id, operation_name) else {
            continue;
        };
        let Some((component, operation)) = find_operation(registry, component_id, operation_name) else {
            if registry.components.contains_key(component_id) {
                model_issue(
                    &span.location,
                    format!("unknown operation '{operation_name}' in '{component_id}'"),
                    issues,
                );
            } else {
                model_issue(&span.location, format!("unknown component '{component_id}'"), issues);
            }
            continue;
        };
        validate_operation(span, component, operation, registry, issues);
    }
}

pub(crate) fn find_operation(
    registry: &RegistrySet,
    component_id: impl AsRef<str>,
    operation_name: impl AsRef<str>,
) -> Option<(&ResolvedComponent, &RegistryOperation)> {
    let component_id = component_id.as_ref();
    let operation_name = operation_name.as_ref();
    let component = registry.components.get(component_id)?;
    let operation = component
        .component
        .operations
        .iter()
        .find(|operation| operation.name == operation_name)?;
    Some((component, operation))
}

fn validate_linkage(span: &TraceSpan, registry: &RegistrySet, issues: &mut Vec<ValidationIssue>) {
    for (key, expected) in [
        (REGISTRY_ATTR, registry.root_id.as_str()),
        (REGISTRY_VERSION_ATTR, registry.root_version.as_str()),
        (REGISTRY_DIGEST_ATTR, registry.root_digest.as_str()),
    ] {
        match span.resource_attributes.get(key).and_then(AnyValue::as_string) {
            Some(actual) if actual == expected => {}
            Some(_) => model_issue(&span.location, format!("trace {key} does not match root registry"), issues),
            None => model_issue(&span.location, format!("missing string attribute '{key}'"), issues),
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
    if let Some(inputs) = span.attributes.get(OP_INPUTS_ATTR).and_then(AnyValue::as_kvlist) {
        let names_match =
            inputs.len() == operation.inputs.len() && operation.inputs.iter().all(|declared| inputs.contains_key(declared.name.as_str()));
        if names_match {
            let max_name_len = operation
                .inputs
                .iter()
                .map(|input| input.name.as_str().len())
                .max()
                .unwrap_or_default();
            let mut location = String::with_capacity(span.location.len() + ".inputs.".len() + max_name_len);
            for declared in &operation.inputs {
                let name = declared.name.as_str();
                location.clear();
                write!(location, "{}.inputs.{name}", span.location).expect("writing to a String cannot fail");
                validate_value(&inputs[name], &declared.value_type, component, registry, &location, issues);
            }
        } else {
            model_issue(
                format!("{}.inputs", span.location),
                "input names do not match registry operation",
                issues,
            );
        }
    }

    let mut event_location = String::with_capacity(span.location.len() + ".events[].value".len() + usize::MAX.ilog10() as usize + 1);
    for (event_index, event) in span.events.iter().enumerate() {
        if event.name != OBS_EVENT {
            continue;
        }
        event_location.clear();
        write!(event_location, "{}.events[{event_index}]", span.location).expect("writing to a String cannot fail");
        let name = event.attributes.get(OBS_NAME_ATTR).and_then(AnyValue::as_string);
        let Some(name) = name else {
            continue;
        };
        let Some(value_type) = operation
            .observations
            .iter()
            .find(|observation| observation.name == name)
            .map(|observation| &observation.value_type)
        else {
            model_issue(&event_location, format!("observation '{name}' is not declared"), issues);
            continue;
        };
        if let Some(value) = event.attributes.get(OBS_VALUE_ATTR) {
            event_location.push_str(".value");
            validate_value(value, value_type, component, registry, &event_location, issues);
        }
    }

    if span.events.iter().any(|event| event.name == FAULT) {
        return;
    }
    let terminal = span
        .events
        .iter()
        .find(|event| matches!(event.name.as_str(), RESULT | EMPTY | ERROR | ABANDONED));
    match terminal.map(|event| event.name.as_str()) {
        None => {
            if operation.outcomes.result.is_some() || operation.outcomes.empty {
                model_issue(&span.location, "unit completion not permitted by registry outcomes", issues);
            }
        }
        Some(RESULT) => {
            let Some(value_type) = operation.outcomes.result.as_ref() else {
                model_issue(&span.location, "result outcome not declared", issues);
                return;
            };
            if let Some(value) = terminal.and_then(|event| event.attributes.get(RESULT_ATTR)) {
                validate_value(value, value_type, component, registry, format!("{}.result", span.location), issues);
            }
        }
        Some(EMPTY) => {
            if !operation.outcomes.empty {
                model_issue(&span.location, "empty outcome not declared", issues);
            }
        }
        Some(ERROR) => {
            let event = terminal.expect("terminal event exists");
            let name = event.attributes.get(ERROR_NAME_ATTR).and_then(AnyValue::as_string);
            let declaration = name.and_then(|name| operation.outcomes.errors.iter().find(|error| error.name == name));
            let Some(declaration) = declaration else {
                model_issue(&span.location, format!("error outcome {name:?} not declared"), issues);
                return;
            };
            match (declaration.value_type.as_ref(), event.attributes.get(ERROR_VALUE_ATTR)) {
                (Some(value_type), Some(value)) => {
                    validate_value(value, value_type, component, registry, format!("{}.error", span.location), issues);
                }
                (Some(_), None) => model_issue(
                    &span.location,
                    format!("error outcome '{}' requires a value", declaration.name),
                    issues,
                ),
                (None, Some(_)) => model_issue(
                    &span.location,
                    format!("error outcome '{}' does not declare a value", declaration.name),
                    issues,
                ),
                (None, None) => {}
            }
        }
        // An operation that never reached an outcome owes the registry no outcome.
        // Abandonment is terminal here for the same reason it is terminal in the
        // trace validator, and reusing the `conformance.fault` early return above
        // would encode "abandonment is a fault", which it is not.
        Some(ABANDONED) => {}
        // Unreachable: `terminal` is selected by the closed list above and every
        // name in it is handled. This arm exists only because the scrutinee is
        // `&str`, which can never be exhaustive. It fires if a name is added to
        // that list without a matching arm here.
        Some(other) => unreachable!("terminal event '{other}' is selected above but not handled"),
    }
}

fn validate_value(
    value: &AnyValue,
    value_type: &TypeRef,
    component: &ResolvedComponent,
    registry: &RegistrySet,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let location = location.as_ref();
    match value_type {
        TypeRef::Primitive { name } => validate_primitive(value, name, location, issues),
        TypeRef::Named {
            name,
            component_id,
            registry_id,
        } => {
            let component_id = component_id.as_deref().unwrap_or(&component.component.id);
            let Some(target) = registry.components.get(component_id) else {
                model_issue(location, format!("unknown component '{component_id}'"), issues);
                return;
            };
            if registry_id.as_ref().is_some_and(|registry_id| registry_id != &target.registry_id) {
                model_issue(
                    location,
                    format!("component '{component_id}' does not belong to registry {registry_id:?}"),
                    issues,
                );
                return;
            }
            let Some(definition) = target.component.types.iter().find(|item| item.name() == name) else {
                model_issue(location, format!("unknown named type '{name}'"), issues);
                return;
            };
            validate_value(value, &definition.as_type(), target, registry, location, issues);
        }
        TypeRef::List { items } | TypeRef::Set { items } => {
            let kind = if matches!(value_type, TypeRef::List { .. }) {
                "list"
            } else {
                "set"
            };
            let AnyValue::Array(values) = value else {
                model_issue(location, format!("{kind} must use {ARRAY_VALUE}"), issues);
                return;
            };
            let before = issues.len();
            let mut item_location = String::with_capacity(index_capacity(location, "[]"));
            for (index, value) in values.iter().enumerate() {
                item_location.clear();
                write!(item_location, "{location}[{index}]").expect("writing to a String cannot fail");
                validate_value(value, items, component, registry, &item_location, issues);
            }
            if kind == "set" && issues.len() == before {
                let canonical = values
                    .iter()
                    .filter_map(|value| canonical_value(value, items, component, registry))
                    .collect::<Vec<_>>();
                if canonical.iter().collect::<BTreeSet<_>>().len() != canonical.len() {
                    model_issue(location, "set contains duplicate elements", issues);
                }
            }
        }
        TypeRef::Map { keys, values } => {
            if matches!(keys.as_ref(), TypeRef::Primitive { name } if name == PRIMITIVE_STRING) {
                let AnyValue::KvList(entries) = value else {
                    model_issue(location, "string-keyed map must use kvlistValue", issues);
                    return;
                };
                let max_key_len = entries.keys().map(String::len).max().unwrap_or_default();
                let mut value_location = String::with_capacity(location.len() + ".".len() + max_key_len);
                for (key, value) in entries {
                    value_location.clear();
                    write!(value_location, "{location}.{key}").expect("writing to a String cannot fail");
                    validate_value(value, values, component, registry, &value_location, issues);
                }
            } else {
                let AnyValue::Array(entries) = value else {
                    model_issue(location, format!("non-string-keyed map must use {ARRAY_VALUE}"), issues);
                    return;
                };
                let mut seen = BTreeSet::new();
                let mut entry_location = String::with_capacity(index_capacity(location, "[].value"));
                for (index, entry) in entries.iter().enumerate() {
                    entry_location.clear();
                    write!(entry_location, "{location}[{index}]").expect("writing to a String cannot fail");
                    let AnyValue::KvList(entry) = entry else {
                        model_issue(&entry_location, "map entry must contain key and value", issues);
                        continue;
                    };
                    if entry.len() != 2 || !entry.contains_key(MAP_KEY) || !entry.contains_key(MAP_VALUE) {
                        model_issue(&entry_location, "map entry must contain key and value", issues);
                        continue;
                    }
                    let entry_base_len = entry_location.len();
                    entry_location.push_str(".key");
                    validate_value(&entry[MAP_KEY], keys, component, registry, &entry_location, issues);
                    entry_location.truncate(entry_base_len);
                    entry_location.push_str(".value");
                    validate_value(&entry[MAP_VALUE], values, component, registry, &entry_location, issues);
                    if let Some(key) = canonical_value(&entry[MAP_KEY], keys, component, registry)
                        && !seen.insert(key)
                    {
                        model_issue(location, "map contains duplicate keys", issues);
                    }
                }
            }
        }
        TypeRef::Tuple { items } => {
            let AnyValue::Array(values) = value else {
                model_issue(location, format!("tuple must use {ARRAY_VALUE}"), issues);
                return;
            };
            if values.len() != items.len() {
                model_issue(location, "tuple arity does not match registry type", issues);
                return;
            }
            let mut item_location = String::with_capacity(index_capacity(location, "[]"));
            for (index, (value, value_type)) in values.iter().zip(items).enumerate() {
                item_location.clear();
                write!(item_location, "{location}[{index}]").expect("writing to a String cannot fail");
                validate_value(value, value_type, component, registry, &item_location, issues);
            }
        }
        TypeRef::Record { fields } => {
            let AnyValue::KvList(entries) = value else {
                model_issue(location, "record must use kvlistValue", issues);
                return;
            };
            let expected = fields.iter().map(|field| field.name.as_str()).collect::<BTreeSet<_>>();
            if entries.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
                model_issue(location, "record fields do not match registry type", issues);
                return;
            }
            let max_name_len = fields.iter().map(|field| field.name.len()).max().unwrap_or_default();
            let mut field_location = String::with_capacity(location.len() + ".".len() + max_name_len);
            for field in fields {
                field_location.clear();
                write!(field_location, "{location}.{}", field.name).expect("writing to a String cannot fail");
                validate_value(
                    &entries[field.name.as_str()],
                    &field.value_type,
                    component,
                    registry,
                    &field_location,
                    issues,
                );
            }
        }
        TypeRef::Optional { value: inner } => {
            let AnyValue::KvList(entries) = value else {
                model_issue(location, "optional must use kvlistValue", issues);
                return;
            };
            if entries.len() != 1 {
                model_issue(location, "optional must contain exactly one variant", issues);
                return;
            }
            let (name, payload) = entries.first_key_value().expect("one entry");
            if name == OPTION_NONE {
                validate_primitive(payload, PRIMITIVE_UNIT, format!("{location}.None"), issues);
            } else if name == OPTION_SOME {
                validate_value(payload, inner, component, registry, format!("{location}.Some"), issues);
            } else {
                model_issue(location, format!("unknown optional variant '{name}'"), issues);
            }
        }
        TypeRef::TaggedUnion { variants } => {
            let AnyValue::KvList(entries) = value else {
                model_issue(location, "tagged union must use kvlistValue", issues);
                return;
            };
            if entries.len() != 1 {
                model_issue(location, "tagged union must contain exactly one variant", issues);
                return;
            }
            let (name, payload) = entries.first_key_value().expect("one entry");
            let Some(variant) = variants.iter().find(|variant| variant.name == *name) else {
                model_issue(location, format!("unknown tagged-union variant '{name}'"), issues);
                return;
            };
            if let Some(value_type) = &variant.payload {
                validate_value(payload, value_type, component, registry, format!("{location}.{name}"), issues);
            } else {
                validate_primitive(payload, PRIMITIVE_UNIT, format!("{location}.{name}"), issues);
            }
        }
    }
}

/// OTLP protobuf-JSON discriminator for key/value-list values.
/// These spellings are serialized interoperability values shared by producers
/// and validators; changing one requires a coordinated CTSC wire migration.
const KVLIST_VALUE: &str = "kvlistValue";
/// OTLP protobuf-JSON discriminator for string values.
const STRING_VALUE: &str = "stringValue";
/// OTLP protobuf-JSON discriminator for array values.
const ARRAY_VALUE: &str = "arrayValue";
/// OTLP protobuf-JSON scalar value discriminators.
const BOOL_VALUE: &str = "boolValue";
const INT_VALUE: &str = "intValue";
const DOUBLE_VALUE: &str = "doubleValue";
const BYTES_VALUE: &str = "bytesValue";
/// Normative key field in CTSC's array representation of non-string map entries.
const MAP_KEY: &str = "key";
/// Normative value field in CTSC's array representation of non-string map entries.
const MAP_VALUE: &str = "value";

fn validate_primitive(value: &AnyValue, primitive: impl AsRef<str>, location: impl AsRef<str>, issues: &mut Vec<ValidationIssue>) {
    let primitive = primitive.as_ref();
    let location = location.as_ref();
    let expected = match primitive {
        PRIMITIVE_UNIT => KVLIST_VALUE,
        PRIMITIVE_STRING | PRIMITIVE_U64 => STRING_VALUE,
        PRIMITIVE_BOOL => BOOL_VALUE,
        PRIMITIVE_I32 | PRIMITIVE_I64 | PRIMITIVE_U32 => INT_VALUE,
        PRIMITIVE_F32 | PRIMITIVE_F64 => DOUBLE_VALUE,
        PRIMITIVE_BYTES => BYTES_VALUE,
        _ => {
            model_issue(location, format!("unsupported primitive '{primitive}'"), issues);
            return;
        }
    };
    let wire_matches = matches!(
        (expected, value),
        (KVLIST_VALUE, AnyValue::KvList(_))
            | (STRING_VALUE, AnyValue::String(_))
            | (BOOL_VALUE, AnyValue::Bool(_))
            | (INT_VALUE, AnyValue::Int(_))
            | (DOUBLE_VALUE, AnyValue::Double(_))
            | (BYTES_VALUE, AnyValue::Bytes(_))
    );
    if !wire_matches {
        model_issue(location, format!("{primitive} must use {expected}"), issues);
        return;
    }
    match (primitive, value) {
        (PRIMITIVE_UNIT, AnyValue::KvList(values)) if !values.is_empty() => {
            model_issue(location, "unit must be an empty kvlistValue", issues);
        }
        (PRIMITIVE_I32, AnyValue::Int(value)) if i32::try_from(*value).is_err() => {
            model_issue(location, "value is outside i32 range", issues);
        }
        (PRIMITIVE_U32, AnyValue::Int(value)) if u32::try_from(*value).is_err() => {
            model_issue(location, "value is outside u32 range", issues);
        }
        (PRIMITIVE_U64, AnyValue::String(value)) => {
            let canonical = value.as_ref() == "0" || (!value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit()));
            if !canonical {
                model_issue(location, "u64 must be a canonical decimal string", issues);
            } else if value.parse::<u64>().is_err() {
                model_issue(location, "value is outside u64 range", issues);
            }
        }
        (PRIMITIVE_F32, AnyValue::Double(F64Value::Finite(value))) => {
            let number = value.get();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "f32 validation deliberately narrows and checks exact round-trip"
            )]
            let narrowed = number as f32;
            if f64::from(narrowed).to_bits() != number.to_bits() {
                model_issue(location, "value is not exactly representable as f32", issues);
            }
        }
        _ => {}
    }
}

pub(crate) fn canonical_value(
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
            canonical_value(value, &definition.as_type(), target, registry)
        }
        TypeRef::List { items } => {
            let AnyValue::Array(values) = value else {
                return None;
            };
            values
                .iter()
                .map(|value| canonical_value(value, items, component, registry))
                .collect::<Option<Vec<_>>>()
                .map(|values| CanonicalValue::List(values.into_boxed_slice()))
        }
        TypeRef::Set { items } => {
            let AnyValue::Array(values) = value else {
                return None;
            };
            values
                .iter()
                .map(|value| canonical_value(value, items, component, registry))
                .collect::<Option<BTreeSet<_>>>()
                .map(CanonicalValue::Set)
        }
        TypeRef::Map { keys, values } => {
            if matches!(keys.as_ref(), TypeRef::Primitive { name } if name == PRIMITIVE_STRING) {
                let AnyValue::KvList(entries) = value else {
                    return None;
                };
                entries
                    .iter()
                    .map(|(key, value)| {
                        canonical_value(value, values, component, registry)
                            .map(|value| (CanonicalValue::String(key.clone().into_boxed_str()), value))
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
                            canonical_value(entry.get(MAP_KEY)?, keys, component, registry)?,
                            canonical_value(entry.get(MAP_VALUE)?, values, component, registry)?,
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
                .map(|(value, value_type)| canonical_value(value, value_type, component, registry))
                .collect::<Option<Vec<_>>>()
                .map(|values| CanonicalValue::Tuple(values.into_boxed_slice()))
        }
        TypeRef::Record { fields } => {
            let AnyValue::KvList(entries) = value else {
                return None;
            };
            fields
                .iter()
                .map(|field| {
                    canonical_value(entries.get(field.name.as_str())?, &field.value_type, component, registry)
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
            let value = if name == OPTION_NONE {
                canonical_primitive(payload, PRIMITIVE_UNIT)?
            } else if name == OPTION_SOME {
                canonical_value(payload, inner, component, registry)?
            } else {
                return None;
            };
            Some(CanonicalValue::Variant(name.clone().into_boxed_str(), Box::new(value)))
        }
        TypeRef::TaggedUnion { variants } => {
            let AnyValue::KvList(entries) = value else {
                return None;
            };
            let (name, payload) = entries.first_key_value()?;
            let variant = variants.iter().find(|variant| variant.name == *name)?;
            let value = match &variant.payload {
                Some(value_type) => canonical_value(payload, value_type, component, registry)?,
                None => canonical_primitive(payload, PRIMITIVE_UNIT)?,
            };
            Some(CanonicalValue::Variant(name.clone().into_boxed_str(), Box::new(value)))
        }
    }
}

fn canonical_primitive(value: &AnyValue, primitive: impl AsRef<str>) -> Option<CanonicalValue> {
    match (primitive.as_ref(), value) {
        (PRIMITIVE_UNIT, AnyValue::KvList(values)) if values.is_empty() => Some(CanonicalValue::Unit),
        (PRIMITIVE_STRING, AnyValue::String(value)) => Some(CanonicalValue::String(value.to_string().into_boxed_str())),
        (PRIMITIVE_BOOL, AnyValue::Bool(value)) => Some(CanonicalValue::Bool(*value)),
        (PRIMITIVE_I32 | PRIMITIVE_I64 | PRIMITIVE_U32, AnyValue::Int(value)) => Some(CanonicalValue::Integer(i128::from(*value))),
        (PRIMITIVE_U64, AnyValue::String(value)) => value.parse::<u64>().ok().map(|value| CanonicalValue::Integer(i128::from(value))),
        (PRIMITIVE_F32 | PRIMITIVE_F64, AnyValue::Double(value)) => Some(CanonicalValue::Float(*value)),
        (PRIMITIVE_BYTES, AnyValue::Bytes(value)) => Some(CanonicalValue::Bytes(value.to_vec().into_boxed_slice())),
        _ => None,
    }
}

fn model_issue(location: impl AsRef<str>, message: impl Into<String>, issues: &mut Vec<ValidationIssue>) {
    issue(location.as_ref().to_string(), message, issues);
}

#[cfg(test)]
mod tests {
    use super::{
        AnyValue, COMPONENT_ATTR, EMPTY, ERROR, ERROR_NAME_ATTR, ERROR_VALUE_ATTR, F64Value, FAULT, MAP_KEY, MAP_VALUE, OBS_EVENT,
        OBS_NAME_ATTR, OBS_VALUE_ATTR, OP_INPUTS_ATTR, OP_NAME_ATTR, OP_SPAN, REGISTRY_ATTR, REGISTRY_DIGEST_ATTR, REGISTRY_VERSION_ATTR,
        RESULT, RESULT_ATTR, RegistrySet, TraceSpan, validate_primitive,
    };
    use crate::validation::model::FiniteF64;

    #[test]
    fn f32_exact_representation() {
        let mut issues = Vec::new();
        validate_primitive(
            &AnyValue::Double(F64Value::Finite(FiniteF64::new(1.5).expect("finite"))),
            "f32",
            "$.value",
            &mut issues,
        );
        validate_primitive(&AnyValue::Double(F64Value::NaN), "f32", "$.nan", &mut issues);
        assert!(issues.is_empty());

        validate_primitive(
            &AnyValue::Double(F64Value::Finite(
                FiniteF64::new(f64::from_bits(1.0_f64.to_bits() + 1)).expect("finite"),
            )),
            "f32",
            "$.inexact",
            &mut issues,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].location, "$.inexact");
        assert_eq!(issues[0].message, "value is not exactly representable as f32");
    }
    #[test]
    fn composites_validate() {
        use super::{RegistrySet, ResolvedComponent, TypeRef, validate_value};
        use crate::validation::model::{RawText, RegistryComponent};
        use std::collections::BTreeMap;

        let component: RegistryComponent = serde_json::from_value(serde_json::json!({
            "id": "demo", "dependencies": [], "operations": [], "types": []
        }))
        .expect("component");
        let component = ResolvedComponent {
            registry_id: RawText::from("registry"),
            component,
        };
        let registry = RegistrySet {
            root_id: RawText::from("registry"),
            root_version: RawText::from("1"),
            root_digest: RawText::from("digest"),
            components: BTreeMap::from([(RawText::from("demo"), component.clone())]),
        };
        let value_type: TypeRef = serde_json::from_value(serde_json::json!({
            "kind": "record",
            "fields": [
                {"name": "items", "type": {"kind": "set", "items": {"kind": "primitive", "name": "i32"}}},
                {"name": "choice", "type": {"kind": "optional", "value": {"kind": "tuple", "items": [
                    {"kind": "primitive", "name": "string"}, {"kind": "primitive", "name": "bool"}
                ]}}}
            ]
        }))
        .expect("type");
        let value = AnyValue::KvList(BTreeMap::from([
            (
                "items".into(),
                AnyValue::Array(vec![AnyValue::Int(1), AnyValue::Int(2)].into_boxed_slice()),
            ),
            (
                "choice".into(),
                AnyValue::KvList(BTreeMap::from([(
                    "Some".into(),
                    AnyValue::Array(vec![AnyValue::String("x".into()), AnyValue::Bool(true)].into_boxed_slice()),
                )])),
            ),
        ]));
        let mut issues = Vec::new();
        validate_value(&value, &value_type, &component, &registry, "$.value", &mut issues);
        assert!(issues.is_empty(), "{issues:?}");

        let duplicate_set = AnyValue::Array(vec![AnyValue::Int(1), AnyValue::Int(1)].into_boxed_slice());
        let set_type: TypeRef = serde_json::from_value(serde_json::json!({
            "kind": "set", "items": {"kind": "primitive", "name": "i32"}
        }))
        .expect("set type");
        validate_value(&duplicate_set, &set_type, &component, &registry, "$.set", &mut issues);
        assert!(issues.iter().any(|issue| issue.message == "set contains duplicate elements"));
    }

    #[test]
    fn primitive_linkage_errors() {
        use super::{CanonicalValue, canonical_primitive};
        assert_eq!(canonical_primitive(&AnyValue::Int(7), "i32"), Some(CanonicalValue::Integer(7)));
        assert_eq!(
            canonical_primitive(&AnyValue::String("9".into()), "u64"),
            Some(CanonicalValue::Integer(9))
        );
        assert_eq!(canonical_primitive(&AnyValue::Bool(true), "string"), None);
        let mut issues = Vec::new();
        validate_primitive(&AnyValue::Bool(true), "string", "$.input", &mut issues);
        assert_eq!(issues[0].location, "$.input");
    }

    fn linked_fixture() -> (RegistrySet, TraceSpan) {
        use crate::validation::model::{RawName, RawText, RegistryComponent, ResolvedComponent, SpanStatus, TraceEvent};
        use std::collections::BTreeMap;

        let component: RegistryComponent = serde_json::from_value(serde_json::json!({
            "id": "demo", "dependencies": [],
            "operations": [{
                "name": "run",
                "inputs": [{"name": "request", "type": {"kind": "primitive", "name": "i32"}}],
                "observations": [{"name": "progress", "type": {"kind": "primitive", "name": "bool"}}],
                "outcomes": {
                    "result": {"kind": "primitive", "name": "string"},
                    "empty": true,
                    "errors": [{"name": "rejected", "type": {"kind": "primitive", "name": "i32"}}]
                }
            }],
            "types": [{"kind": "record", "name": "Envelope", "fields": [
                {"name": "value", "type": {"kind": "primitive", "name": "string"}}
            ]}]
        }))
        .expect("linked fixture component");
        let resolved = ResolvedComponent {
            registry_id: RawText::from("registry"),
            component,
        };
        let registry = RegistrySet {
            root_id: RawText::from("registry"),
            root_version: RawText::from("1"),
            root_digest: RawText::from("digest"),
            components: BTreeMap::from([(RawText::from("demo"), resolved)]),
        };
        let span = TraceSpan {
            trace_id: RawText::from("trace"),
            span_id: RawText::from("span"),
            parent_span_id: RawText::from("parent"),
            name: RawName::from(OP_SPAN),
            start_time: Some(1),
            end_time: Some(2),
            status: SpanStatus::Unset,
            attributes: BTreeMap::from([
                (COMPONENT_ATTR.into(), AnyValue::String("demo".into())),
                (OP_NAME_ATTR.into(), AnyValue::String("run".into())),
                (
                    OP_INPUTS_ATTR.into(),
                    AnyValue::KvList(BTreeMap::from([("request".into(), AnyValue::Int(7))])),
                ),
            ]),
            events: vec![
                TraceEvent {
                    name: RawName::from(OBS_EVENT),
                    attributes: BTreeMap::from([
                        (OBS_NAME_ATTR.into(), AnyValue::String("progress".into())),
                        (OBS_VALUE_ATTR.into(), AnyValue::Bool(true)),
                    ]),
                },
                TraceEvent {
                    name: RawName::from(RESULT),
                    attributes: BTreeMap::from([(RESULT_ATTR.into(), AnyValue::String("done".into()))]),
                },
            ]
            .into_boxed_slice(),
            resource_attributes: BTreeMap::from([
                (REGISTRY_ATTR.into(), AnyValue::String("registry".into())),
                (REGISTRY_VERSION_ATTR.into(), AnyValue::String("1".into())),
                (REGISTRY_DIGEST_ATTR.into(), AnyValue::String("digest".into())),
            ]),
            location: "$.spans[0]".into(),
        };
        (registry, span)
    }

    #[test]
    fn linkage_channels() {
        use super::{TraceDocument, check_linked};
        use crate::validation::model::{RawName, TraceEvent};
        use std::collections::BTreeMap;

        let (registry, span) = linked_fixture();
        let mut issues = Vec::new();
        check_linked(&TraceDocument { spans: vec![span.clone()] }, &registry, &mut issues);
        assert!(issues.is_empty(), "{issues:?}");

        for event in [
            TraceEvent {
                name: RawName::from(EMPTY),
                attributes: BTreeMap::new(),
            },
            TraceEvent {
                name: RawName::from(ERROR),
                attributes: BTreeMap::from([
                    (ERROR_NAME_ATTR.into(), AnyValue::String("rejected".into())),
                    (ERROR_VALUE_ATTR.into(), AnyValue::Int(9)),
                ]),
            },
            TraceEvent {
                name: RawName::from(FAULT),
                attributes: BTreeMap::new(),
            },
        ] {
            let mut variant = span.clone();
            variant.events = vec![event].into_boxed_slice();
            issues.clear();
            check_linked(&TraceDocument { spans: vec![variant] }, &registry, &mut issues);
            assert!(issues.is_empty(), "{issues:?}");
        }

        let mut invalid = span;
        invalid.resource_attributes.remove(REGISTRY_DIGEST_ATTR);
        invalid.attributes.insert(OP_INPUTS_ATTR.into(), AnyValue::KvList(BTreeMap::new()));
        invalid.events[0]
            .attributes
            .insert(OBS_NAME_ATTR.into(), AnyValue::String("unknown".into()));
        invalid.events[1].attributes.insert(RESULT_ATTR.into(), AnyValue::Bool(false));
        check_linked(&TraceDocument { spans: vec![invalid] }, &registry, &mut issues);
        let messages = issues.iter().map(|issue| issue.message.as_str()).collect::<Vec<_>>();
        assert!(messages.iter().any(|message| message.contains("missing string attribute")));
        assert!(messages.contains(&"input names do not match registry operation"));
        assert!(messages.contains(&"observation 'unknown' is not declared"));
        assert!(messages.contains(&"string must use stringValue"));
    }

    #[test]
    fn value_shapes() {
        use super::{TypeRef, validate_value};
        use std::collections::BTreeMap;

        let (registry, _) = linked_fixture();
        let component = &registry.components["demo"];
        let cases = [
            (
                serde_json::json!({"kind": "named", "name": "Envelope"}),
                AnyValue::KvList(BTreeMap::from([("value".into(), AnyValue::String("ok".into()))])),
            ),
            (
                serde_json::json!({"kind": "map", "keys": {"kind": "primitive", "name": "string"}, "values": {"kind": "primitive", "name": "bool"}}),
                AnyValue::KvList(BTreeMap::from([("enabled".into(), AnyValue::Bool(true))])),
            ),
            (
                serde_json::json!({"kind": "map", "keys": {"kind": "primitive", "name": "i32"}, "values": {"kind": "primitive", "name": "string"}}),
                AnyValue::Array(
                    vec![AnyValue::KvList(BTreeMap::from([
                        (MAP_KEY.into(), AnyValue::Int(1)),
                        (MAP_VALUE.into(), AnyValue::String("one".into())),
                    ]))]
                    .into_boxed_slice(),
                ),
            ),
            (
                serde_json::json!({"kind": "tagged_union", "variants": [
                    {"name": "Text", "payload": {"kind": "primitive", "name": "string"}}, {"name": "None"}
                ]}),
                AnyValue::KvList(BTreeMap::from([("Text".into(), AnyValue::String("value".into()))])),
            ),
        ];
        for (shape, value) in cases {
            let value_type: TypeRef = serde_json::from_value(shape).expect("value shape");
            let mut issues = Vec::new();
            validate_value(&value, &value_type, component, &registry, "$.value", &mut issues);
            assert!(issues.is_empty(), "{issues:?}");
        }
    }

    #[test]
    fn canonical_failures() {
        use super::{TypeRef, canonical_value, validate_value};
        use std::collections::BTreeMap;

        let (registry, _) = linked_fixture();
        let component = &registry.components["demo"];
        let malformed = [
            (
                serde_json::json!({"kind": "tuple", "items": [{"kind": "primitive", "name": "i32"}]}),
                AnyValue::Bool(true),
            ),
            (
                serde_json::json!({"kind": "record", "fields": [{"name": "field", "type": {"kind": "primitive", "name": "string"}}]}),
                AnyValue::KvList(BTreeMap::new()),
            ),
            (
                serde_json::json!({"kind": "optional", "value": {"kind": "primitive", "name": "string"}}),
                AnyValue::KvList(BTreeMap::from([("Other".into(), AnyValue::String("x".into()))])),
            ),
            (
                serde_json::json!({"kind": "tagged_union", "variants": [{"name": "Known"}]}),
                AnyValue::KvList(BTreeMap::from([("Unknown".into(), AnyValue::KvList(BTreeMap::new()))])),
            ),
        ];
        for (shape, value) in malformed {
            let value_type: TypeRef = serde_json::from_value(shape).expect("malformed-case type");
            let mut issues = Vec::new();
            validate_value(&value, &value_type, component, &registry, "$.bad", &mut issues);
            assert!(!issues.is_empty());
            assert_eq!(canonical_value(&value, &value_type, component, &registry), None);
        }
    }
}
