use super::*;

pub(super) fn parse_attributes(
    value: Option<&Value>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> BTreeMap<String, AnyValue> {
    let path = path.as_ref();
    let location = location.as_ref();
    let mut result = BTreeMap::new();
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return result;
    };
    let Some(attributes) = value.as_array() else {
        issue(
            located(path, format!("{location}.attributes")),
            "attributes must be an array",
            issues,
        );
        return result;
    };
    let mut value_location = String::new();
    let mut item_location = String::with_capacity(location.len() + ".attributes[]".len() + usize::MAX.ilog10() as usize + 1);
    for (index, attribute) in attributes.iter().enumerate() {
        item_location.clear();
        write!(item_location, "{location}.attributes[{index}]").expect("writing to a String cannot fail");
        let Some(attribute) = attribute.as_object() else {
            issue(located(path, &item_location), "attribute must be an object", issues);
            continue;
        };
        let Some(key) = json_field(attribute, "key").and_then(Value::as_str) else {
            issue(located(path, &item_location), "attribute key must be a string", issues);
            continue;
        };
        if result.contains_key(key) {
            issue(located(path, location), format!("duplicate attribute key '{key}'"), issues);
            continue;
        }
        let Some(raw_value) = json_field(attribute, "value") else {
            issue(
                located(path, format!("{location}.{key}")),
                "attribute value must be an AnyValue",
                issues,
            );
            continue;
        };
        value_location.clear();
        write!(value_location, "{location}.{key}").expect("writing to a String cannot fail");
        if let Some(value) = parse_value(raw_value, path, &value_location, issues) {
            result.insert(key.to_string(), value);
        }
    }
    result
}

pub(super) fn parse_value(
    value: &Value,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<AnyValue> {
    let path = path.as_ref();
    let location = location.as_ref();
    let Some(value) = value.as_object() else {
        issue(located(path, location), "AnyValue must be an object", issues);
        return None;
    };
    let mut selected = None;
    let mut count = 0_usize;
    for key in VALUE_KEYS {
        if json_field(value, key).is_some() {
            selected = Some(key);
            count += 1;
        }
    }
    if count != 1 || value.len() != 1 {
        issue(
            located(path, location),
            "AnyValue must select exactly one concrete value variant",
            issues,
        );
        return None;
    }
    let selected = selected.expect("exactly one AnyValue field was counted");
    let raw = json_field(value, selected).expect("selected AnyValue field");
    match selected {
        "stringValue" => raw.as_str().map(|value| AnyValue::String(value.into())).or_else(|| {
            issue(located(path, location), "stringValue must be a string", issues);
            None
        }),
        "boolValue" => raw.as_bool().map(AnyValue::Bool).or_else(|| {
            issue(located(path, location), "boolValue must be a boolean", issues);
            None
        }),
        "intValue" => otlp::parse_i64(raw).map(AnyValue::Int).or_else(|| {
            issue(
                located(path, location),
                "intValue must contain an OTLP int64 JSON number or numeric string",
                issues,
            );
            None
        }),
        "doubleValue" => parse_double(raw, path, location, issues).map(AnyValue::Double),
        "bytesValue" => raw
            .as_str()
            .and_then(otlp::decode_base64)
            .map(|value| AnyValue::Bytes(value.into_boxed_slice()))
            .or_else(|| {
                issue(located(path, location), "bytesValue must be valid base64", issues);
                None
            }),
        "arrayValue" => {
            let Some(array) = raw.as_object() else {
                issue(located(path, location), "arrayValue must be an object", issues);
                return None;
            };
            let values = optional_array(array, "values", path, location, issues)?;
            let mut parsed = Vec::with_capacity(values.len());
            let mut item_location = String::with_capacity(location.len() + "[]".len() + usize::MAX.ilog10() as usize + 1);
            for (index, item) in values.iter().enumerate() {
                item_location.clear();
                write!(item_location, "{location}[{index}]").expect("writing to a String cannot fail");
                if let Some(item) = parse_value(item, path, &item_location, issues) {
                    parsed.push(item);
                }
            }
            parsed.shrink_to_fit();
            Some(AnyValue::Array(parsed.into_boxed_slice()))
        }
        "kvlistValue" => {
            let Some(kvlist) = raw.as_object() else {
                issue(located(path, location), "kvlistValue must be an object", issues);
                return None;
            };
            let values = optional_array(kvlist, "values", path, location, issues)?;
            let mut parsed = BTreeMap::new();
            let mut item_location = String::with_capacity(location.len() + ".".len() + usize::MAX.ilog10() as usize + 1);
            for (index, item) in values.iter().enumerate() {
                item_location.clear();
                write!(item_location, "{location}.{index}").expect("writing to a String cannot fail");
                let Some(item) = item.as_object() else {
                    issue(located(path, &item_location), "kvlist item must be an object", issues);
                    continue;
                };
                let Some(key) = json_field(item, "key").and_then(Value::as_str) else {
                    issue(located(path, &item_location), "kvlist item must contain a string key", issues);
                    continue;
                };
                if parsed.contains_key(key) {
                    issue(located(path, location), format!("duplicate kvlist key '{key}'"), issues);
                    continue;
                }
                let Some(child) = json_field(item, "value") else {
                    issue(located(path, &item_location), "kvlist item must contain an AnyValue", issues);
                    continue;
                };
                item_location.clear();
                write!(item_location, "{location}.{key}").expect("writing to a String cannot fail");
                if let Some(child) = parse_value(child, path, &item_location, issues) {
                    parsed.insert(key.to_string(), child);
                }
            }
            Some(AnyValue::KvList(parsed))
        }
        _ => unreachable!("AnyValue key selection must match exactly one supported OTLP value variant"),
    }
}

pub(super) fn parse_double(
    value: &Value,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<F64Value> {
    let path = path.as_ref();
    let location = location.as_ref();
    match otlp::parse_double(value) {
        Some(ParsedDouble::Finite(value)) => FiniteF64::new(value.get()).map(F64Value::Finite),
        Some(ParsedDouble::NaN) => Some(F64Value::NaN),
        Some(ParsedDouble::Infinity) => Some(F64Value::Infinity),
        Some(ParsedDouble::NegativeInfinity) => Some(F64Value::NegativeInfinity),
        None => {
            issue(
                located(path, location),
                "doubleValue must be an OTLP number or numeric/symbolic string",
                issues,
            );
            None
        }
    }
}

pub(super) fn array_field<'a>(
    object: &'a Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> &'a [Value] {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    match json_field(object, key) {
        None | Some(Value::Null) => &[],
        Some(Value::Array(values)) => values,
        Some(_) => {
            issue(
                located(path, format!("{location}.{key}")),
                format!("{key} must be an array"),
                issues,
            );
            &[]
        }
    }
}

pub(super) fn optional_array<'a>(
    object: &'a Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<&'a [Value]> {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    match json_field(object, key) {
        None | Some(Value::Null) => Some(&[]),
        Some(Value::Array(values)) => Some(values),
        Some(_) => {
            issue(
                located(path, location),
                format!("{} must contain a values array", if key == "values" { "value" } else { key }),
                issues,
            );
            None
        }
    }
}

pub(super) fn json_field(object: &Map<String, Value>, key: impl AsRef<str>) -> Option<&Value> {
    let key = key.as_ref();
    let alternate = match key {
        "stringValue" => "string_value",
        "boolValue" => "bool_value",
        "intValue" => "int_value",
        "doubleValue" => "double_value",
        "bytesValue" => "bytes_value",
        "arrayValue" => "array_value",
        "kvlistValue" => "kvlist_value",
        "resourceSpans" => "resource_spans",
        "scopeSpans" => "scope_spans",
        "traceId" => "trace_id",
        "spanId" => "span_id",
        "parentSpanId" => "parent_span_id",
        "startTimeUnixNano" => "start_time_unix_nano",
        "endTimeUnixNano" => "end_time_unix_nano",
        "droppedAttributesCount" => "dropped_attributes_count",
        "droppedEventsCount" => "dropped_events_count",
        "droppedLinksCount" => "dropped_links_count",
        _ => key,
    };
    object
        .get(key)
        .or_else(|| (alternate != key).then(|| object.get(alternate)).flatten())
}

pub(super) fn string_field(
    object: &Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    if let Some(value) = json_field(object, key).and_then(Value::as_str) {
        Some(value.to_string())
    } else {
        issue(
            located(path, format!("{location}.{key}")),
            format!("{key} must be a string"),
            issues,
        );
        None
    }
}

pub(super) fn optional_string(
    object: &Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    match json_field(object, key) {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            issue(
                located(path, format!("{location}.{key}")),
                format!("{key} must be a string"),
                issues,
            );
            None
        }
    }
}

pub(super) fn id_field(
    object: &Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    let value = string_field(object, key, path, location, issues)?;
    otlp::normalize_hex(&value).or_else(|| {
        issue(
            located(path, format!("{location}.{key}")),
            format!("{key} must use the OTLP hexadecimal bytes mapping"),
            issues,
        );
        None
    })
}

pub(super) fn optional_id(
    object: &Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    let value = optional_string(object, key, path, location, issues)?;
    otlp::normalize_hex(&value).or_else(|| {
        issue(
            located(path, format!("{location}.{key}")),
            format!("{key} must use the OTLP hexadecimal bytes mapping"),
            issues,
        );
        None
    })
}

pub(super) fn time_field(
    object: &Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<u128> {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    let value = json_field(object, key)?;
    if value.is_null() {
        return None;
    }
    let parsed = otlp::parse_u64(value).map(u128::from);
    if parsed.is_none() {
        issue(
            located(path, format!("{location}.{key}")),
            format!("{key} must be an unsigned decimal integer"),
            issues,
        );
    }
    parsed
}

pub(super) fn validate_zero(
    object: &Map<String, Value>,
    key: impl AsRef<str>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let key = key.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    let Some(value) = json_field(object, key).filter(|value| !value.is_null()) else {
        return;
    };
    let parsed = otlp::parse_u32(value);
    require(
        parsed == Some(0),
        located(path, location),
        format!("{key} must be a valid uint32 zero"),
        issues,
    );
}

#[derive(Clone, Copy)]
pub(super) enum AttributeType {
    String,
    Int,
}

pub(super) fn reject_attrs<'a>(
    attributes: &BTreeMap<String, AnyValue>,
    allowed: impl AsRef<[&'a str]>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let allowed = allowed.as_ref();
    let location = location.as_ref();
    for key in attributes.keys().filter(|key| key.starts_with("conformance.")) {
        if !allowed.contains(&key.as_str()) {
            issue(
                location.to_string(),
                format!("undeclared CTSC attribute '{key}' is not allowed in this scope"),
                issues,
            );
        }
    }
}

pub(super) fn check_attr(
    attributes: &BTreeMap<String, AnyValue>,
    key: impl AsRef<str>,
    expected: AttributeType,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let key = key.as_ref();
    let location = location.as_ref();
    let Some(value) = attributes.get(key) else {
        return;
    };
    let valid = match expected {
        AttributeType::String => matches!(value, AnyValue::String(_)),
        AttributeType::Int => matches!(value, AnyValue::Int(_)),
    };
    if !valid {
        let variant = match expected {
            AttributeType::String => "stringValue",
            AttributeType::Int => "intValue",
        };
        issue(location.to_string(), format!("attribute '{key}' must use {variant}"), issues);
    }
}

pub(super) fn require_str<'a>(
    attributes: &'a BTreeMap<String, AnyValue>,
    key: impl AsRef<str>,
    _path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<&'a str> {
    let key = key.as_ref();
    let location = location.as_ref();
    match attributes.get(key) {
        Some(AnyValue::String(value)) => Some(value),
        Some(_) => {
            issue(location.to_string(), format!("attribute '{key}' must use stringValue"), issues);
            None
        }
        None => {
            issue(location.to_string(), format!("missing string attribute '{key}'"), issues);
            None
        }
    }
}

pub(super) fn is_hex_id(value: impl AsRef<str>, length: usize) -> bool {
    let value = value.as_ref();
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn require(condition: bool, location: impl AsRef<str>, message: impl AsRef<str>, issues: &mut Vec<ValidationIssue>) {
    if !condition {
        issue(location.as_ref().to_string(), message.as_ref(), issues);
    }
}
