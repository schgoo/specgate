//! Typed CTSC value decoding from OTLP `AnyValue` payloads.

use super::{AnyValue, DoubleValue, Type, Value, error};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decode(value: serde_json::Value, ty: &Type) -> Result<Value, error::Error> {
        decode_value(&serde_json::from_value::<AnyValue>(value).unwrap(), ty, "input")
    }

    #[test]
    fn recursive_inputs_preserve_unsigned_ranges_and_float_bits() {
        let ty = Type::map(Type::primitive("string"), Type::optional(Type::list(Type::primitive("u64"))));
        let decoded = decode(
            json!({"kvlistValue":{"values":[
                {"key":"present","value":{"kvlistValue":{"values":[
                    {"key":"Some","value":{"arrayValue":{"values":[{"stringValue":"18446744073709551615"}]}}}
                ]}}},
                {"key":"absent","value":{"kvlistValue":{"values":[
                    {"key":"None","value":{"kvlistValue":{"values":[]}}}
                ]}}}
            ]}}),
            &ty,
        )
        .unwrap();
        let Value::Map(values) = decoded else {
            panic!("expected decoded map")
        };
        assert_eq!(
            values["present"],
            Value::Optional(Some(Box::new(Value::List(vec![Value::U64(u64::MAX)]))))
        );
        assert_eq!(values["absent"], Value::Optional(None));
        let decoded = decode(
            json!({"arrayValue":{"values":[{"doubleValue":-0.0}]}}),
            &Type::list(Type::primitive("f64")),
        )
        .unwrap();
        assert_eq!(decoded, Value::List(vec![Value::F64Bits((-0.0_f64).to_bits())]));
    }

    #[test]
    fn malformed_collection_inputs_report_precise_paths() {
        let invalid = json!({"arrayValue":{"values":[{"intValue":"-1"}]}});
        let error = decode(invalid, &Type::list(Type::primitive("u32"))).unwrap_err().to_string();
        assert!(error.contains("input[0]") && error.contains("range"), "{error}");
        let duplicates = json!({"arrayValue":{"values":[{"intValue":"1"},{"intValue":"1"}]}});
        assert!(
            decode(duplicates, &Type::set(Type::primitive("i32")))
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
        let duplicates = json!({"kvlistValue":{"values":[
            {"key":"a","value":{"intValue":"1"}},{"key":"a","value":{"intValue":"2"}}
        ]}});
        decode(duplicates, &Type::map(Type::primitive("string"), Type::primitive("i32"))).unwrap_err();
        for invalid in [
            json!({"kvlistValue":{"values":[]}}),
            json!({"kvlistValue":{"values":[{"key":"Other","value":{"intValue":"1"}}]}}),
            json!({"kvlistValue":{"values":[{"key":"None","value":{"intValue":"1"}}]}}),
        ] {
            decode(invalid, &Type::optional(Type::primitive("i32"))).unwrap_err();
        }
    }
}

pub(super) fn decode_value(value: &AnyValue, value_type: &Type, location: impl AsRef<str>) -> Result<Value, error::Error> {
    let location = location.as_ref();
    if let Some(item_type) = value_type.element_type() {
        let AnyValue::Array(array) = value else {
            return Err(format!("{location} collection value must use arrayValue").into());
        };
        let values = array
            .values
            .iter()
            .enumerate()
            .map(|(index, item)| decode_value(item, item_type, format!("{location}[{index}]")))
            .collect::<Result<Vec<_>, _>>()?;
        if value_type.kind_name() == "set" && values.iter().enumerate().any(|(index, value)| values[..index].contains(value)) {
            return Err(format!("{location} has duplicate set elements").into());
        }
        return Ok(if value_type.kind_name() == "set" {
            Value::Set(values)
        } else {
            Value::List(values)
        });
    }
    if let Some(inner) = value_type.optional_value() {
        let AnyValue::Kvlist(list) = value else {
            return Err(format!("{location} optional must use a single-key kvlistValue").into());
        };
        if list.values.len() != 1 {
            return Err(format!("{location} optional must have exactly one Some or None tag").into());
        }
        let entry = &list.values[0];
        return match entry.key.as_ref() {
            "Some" => decode_value(&entry.value, inner, format!("{location}.Some")).map(|value| Value::Optional(Some(Box::new(value)))),
            "None" => {
                decode_value(&entry.value, &Type::primitive("unit"), format!("{location}.None"))?;
                Ok(Value::Optional(None))
            }
            _ => Err(format!("{location} optional must use Some or None").into()),
        };
    }
    if let Some((keys, values)) = value_type.map_types() {
        if keys.primitive_name() != Some("string") {
            return Err(format!("{location} replay map keys must be strings").into());
        }
        let AnyValue::Kvlist(list) = value else {
            return Err(format!("{location} map must use kvlistValue").into());
        };
        let mut result = std::collections::BTreeMap::new();
        for entry in &list.values {
            let decoded = decode_value(&entry.value, values, format!("{location}[{:?}]", entry.key))?;
            if result.insert(entry.key.to_string(), decoded).is_some() {
                return Err(format!("{location} has duplicate map key {:?}", entry.key).into());
            }
        }
        return Ok(Value::Map(result));
    }
    let Some(name) = value_type.primitive_name() else {
        return Err(format!("{location} uses unsupported structured replay type {}", type_name(value_type)).into());
    };
    match name {
        "unit" => {
            let AnyValue::Kvlist(list) = value else {
                return Err(format!("{location} unit value must use kvlistValue").into());
            };
            if !list.values.is_empty() {
                return Err(format!("{location} unit value must be an empty kvlistValue").into());
            }
            Ok(Value::Unit)
        }
        "string" => match value {
            AnyValue::String(value) => Ok(Value::String(value.clone())),
            _ => Err(format!("{location} string value must use stringValue").into()),
        },
        "bool" => match value {
            AnyValue::Bool(value) => Ok(Value::Bool(*value)),
            _ => Err(format!("{location} bool value must use boolValue").into()),
        },
        "i32" => {
            let value = decode_integer(value, i64::from(i32::MIN)..=i64::from(i32::MAX), location)?;
            Ok(Value::I32(i32::try_from(value).expect("i32 replay range was checked")))
        }
        "i64" => decode_integer(value, i64::MIN..=i64::MAX, location).map(Value::I64),
        "u32" => {
            let value = decode_integer(value, 0..=i64::from(u32::MAX), location)?;
            Ok(Value::U32(u32::try_from(value).expect("u32 replay range was checked")))
        }
        "u64" => {
            let AnyValue::String(value) = value else {
                return Err(format!("{location} u64 value must use stringValue").into());
            };
            let parsed = value
                .parse::<u64>()
                .map_err(|error| format!("{location} is not a valid u64 decimal string: {error}"))?;
            if parsed.to_string() != *value {
                return Err(format!("{location} u64 value must use canonical unsigned decimal text").into());
            }
            Ok(Value::U64(parsed))
        }
        "f32" => decode_float(value, FloatWidth::F32, location),
        "f64" => decode_float(value, FloatWidth::F64, location),
        "bytes" => Err(format!("{location} uses unsupported replay primitive 'bytes'").into()),
        other => Err(format!("{location} uses unknown CTSC primitive '{other}'").into()),
    }
}

fn decode_integer(value: &AnyValue, range: std::ops::RangeInclusive<i64>, location: impl AsRef<str>) -> Result<i64, error::Error> {
    let location = location.as_ref();
    let AnyValue::Int(value) = value else {
        return Err(format!("{location} integer value must use intValue").into());
    };
    let parsed = value
        .parse::<i64>()
        .map_err(|error| format!("{location} is not a signed decimal integer: {error}"))?;
    if !range.contains(&parsed) {
        return Err(format!(
            "{location} integer value {parsed} is outside supported range {}..={}",
            range.start(),
            range.end()
        )
        .into());
    }
    Ok(parsed)
}

#[derive(Clone, Copy)]
enum FloatWidth {
    F32,
    F64,
}

fn decode_float(value: &AnyValue, width: FloatWidth, location: impl AsRef<str>) -> Result<Value, error::Error> {
    let location = location.as_ref();
    let AnyValue::Double(value) = value else {
        return Err(format!("{location} floating-point value must use doubleValue").into());
    };
    match value {
        DoubleValue::Number(value) if matches!(width, FloatWidth::F32) => {
            let narrowed = value
                .to_string()
                .parse::<f32>()
                .map_err(|error| format!("{location} value is outside f32 range: {error}"))?;
            if f64::from(narrowed).to_bits() != value.to_bits() {
                return Err(format!("{location} value is not exactly representable as f32").into());
            }
            Ok(Value::F32Bits(narrowed.to_bits()))
        }
        DoubleValue::Number(value) => Ok(Value::F64Bits(value.to_bits())),
        DoubleValue::Symbol(symbol) => {
            let bits = match (width, symbol.as_str()) {
                (FloatWidth::F32, "NaN") => return Ok(Value::F32Bits(f32::NAN.to_bits())),
                (FloatWidth::F32, "Infinity") => return Ok(Value::F32Bits(f32::INFINITY.to_bits())),
                (FloatWidth::F32, "-Infinity") => return Ok(Value::F32Bits(f32::NEG_INFINITY.to_bits())),
                (FloatWidth::F64, "NaN") => f64::NAN.to_bits(),
                (FloatWidth::F64, "Infinity") => f64::INFINITY.to_bits(),
                (FloatWidth::F64, "-Infinity") => f64::NEG_INFINITY.to_bits(),
                _ => return Err(format!("{location} contains invalid symbolic doubleValue '{symbol}'").into()),
            };
            Ok(Value::F64Bits(bits))
        }
    }
}

fn type_name(value_type: &Type) -> &'static str {
    value_type.kind_name()
}
