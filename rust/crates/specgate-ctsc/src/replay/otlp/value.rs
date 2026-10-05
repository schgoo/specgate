//! Typed CTSC value decoding from OTLP `AnyValue` payloads.

use super::{AnyValue, DoubleValue, Type, Value, error};

pub(super) fn decode_value(value: &AnyValue, value_type: &Type, location: impl AsRef<str>) -> Result<Value, error::Error> {
    let location = location.as_ref();
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
