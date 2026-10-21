//! Internal bridge used by generated Cargo test-harness entry points.

use crate::{CaptureError, CaptureErrorKind, capture::Capture, registry, value::Value};
use std::any::Any;

/// Type-erased synchronous constructor with explicit input failures.
pub type Constructor = fn(Vec<Value>) -> Result<Box<dyn Any>, CaptureError>;

/// Erased setup constructor, emitted in the producer's own lexical scope.
#[derive(Debug, Clone, Copy)]
pub struct Setup {
    invoke: Constructor,
    output: fn() -> &'static str,
}

impl Setup {
    /// Construct a generated setup bridge.
    #[must_use]
    pub const fn new(invoke: Constructor, output: fn() -> &'static str) -> Self {
        Self { invoke, output }
    }

    pub(crate) fn output(self) -> &'static str {
        (self.output)()
    }
}

/// Construct an explicit generated-harness failure.
#[must_use]
pub fn error(message: impl Into<String>) -> CaptureError {
    CaptureError::message(CaptureErrorKind::Configuration, message)
}

/// Invoke exactly one annotated producer in the current test target.
///
/// # Errors
/// Rejects absent, ambiguous, or unsupported producers and invalid inputs.
pub fn setup(
    component: &str,
    operation: &str,
    module: &str,
    function: &str,
    fills: &str,
    inputs: Vec<Value>,
) -> Result<Box<dyn Any>, CaptureError> {
    let matches = registry::OPERATIONS
        .iter()
        .filter(|entry| {
            entry.is_setup
                && entry.component == component
                && entry.name == operation
                && entry.module_path == module
                && entry.fn_name == function
                && entry.fills.unwrap_or_default() == fills
        })
        .collect::<Vec<_>>();
    let entry = match matches.as_slice() {
        [entry] => *entry,
        [] => {
            return Err(error(format!(
                "test target does not contain setup '{component}::{operation}' at '{module}::{function}'"
            )));
        }
        _ => {
            return Err(error(format!(
                "test target duplicates setup '{component}::{operation}' at '{module}::{function}'"
            )));
        }
    };
    let bridge = entry
        .replay_setup
        .ok_or_else(|| error(format!("setup '{module}::{function}' has no supported replay bridge")))?;
    (bridge.invoke)(inputs)
}

/// Primitive construction-input conversion used by generated setup bridges.
pub trait Input: Sized {
    /// Decode one native construction input.
    ///
    /// # Errors
    /// Rejects a mismatched kind or an out-of-range number.
    fn decode(value: Value) -> Result<Self, CaptureError>;
}

macro_rules! input {
    ($ty:ty, $variant:ident, $convert:expr) => {
        impl Input for $ty {
            fn decode(value: Value) -> Result<Self, CaptureError> {
                match value {
                    Value::$variant(value) => {
                        ($convert)(value).map_err(|message| error(format!("invalid {} setup input: {message}", stringify!($ty))))
                    }
                    other => Err(error(format!(
                        "expected {} setup input, got {}",
                        stringify!($ty),
                        other.type_name()
                    ))),
                }
            }
        }
    };
}

input!(String, String, Ok::<_, std::convert::Infallible>);
input!(bool, Bool, Ok::<_, std::convert::Infallible>);
input!(i32, Integer, i32::try_from);
input!(i64, Integer, Ok::<_, std::convert::Infallible>);
input!(u32, Unsigned, u32::try_from);
input!(u64, Unsigned, Ok::<_, std::convert::Infallible>);
input!(f64, Float, Ok::<_, std::convert::Infallible>);

impl Input for f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the planner verifies the exact f32 bit projection before invoking this bridge"
    )]
    fn decode(value: Value) -> Result<Self, CaptureError> {
        match value {
            Value::Float(value) if f64::from(value as Self).to_bits() == value.to_bits() => Ok(value as Self),
            _ => Err(error("expected an exactly representable f32 setup input")),
        }
    }
}

impl Input for () {
    fn decode(value: Value) -> Result<Self, CaptureError> {
        match value {
            Value::List(values) if values.is_empty() => Ok(()),
            _ => Err(error("expected a unit setup input")),
        }
    }
}

/// Decode the next construction input without silently supplying missing values.
///
/// # Errors
/// Rejects missing or ill-typed construction inputs.
pub fn argument<T: Input>(values: &mut impl Iterator<Item = Value>, name: &str) -> Result<T, CaptureError> {
    T::decode(values.next().ok_or_else(|| error(format!("missing setup input '{name}'")))?)
}

/// Run a directly awaited replay on a timer- and I/O-capable executor.
///
/// # Errors
/// Reports executor construction failures.
pub fn run<F: Future>(future: F) -> Result<F::Output, CaptureError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| CaptureError::caused(CaptureErrorKind::Configuration, "failed to start replay executor", source))?;
    Ok(runtime.block_on(future))
}

/// Serialize completed captures for the CLI's existing publication pipeline.
///
/// # Errors
/// Reports serialization and filesystem failures.
pub fn publish(path: &std::path::Path, captures: &[Capture]) -> Result<(), CaptureError> {
    let bytes = serde_json::to_vec(captures)
        .map_err(|source| CaptureError::caused(CaptureErrorKind::Configuration, "failed to encode test-harness captures", source))?;
    std::fs::write(path, bytes)
        .map_err(|source| CaptureError::caused(CaptureErrorKind::Persistence, "failed to write test-harness captures", source))
}

#[cfg(test)]
mod tests {
    use super::{Input, Value, argument};

    #[test]
    fn construction_inputs_are_lossless_and_fail_explicitly() {
        assert_eq!(i32::decode(Value::Integer(7)).unwrap(), 7);
        i32::decode(Value::Integer(i64::MAX)).unwrap_err();
        u32::decode(Value::Unsigned(u64::MAX)).unwrap_err();
        u32::decode(Value::Integer(-1)).unwrap_err();
        String::decode(Value::Bool(true)).unwrap_err();
        assert_eq!(f32::decode(Value::Float(0.5)).unwrap().to_bits(), 0.5_f32.to_bits());
        f32::decode(Value::Float(0.1)).unwrap_err();
        <()>::decode(Value::List(vec![Value::Integer(1)])).unwrap_err();
        let error = argument::<i32>(&mut std::iter::empty(), "initial").unwrap_err();
        assert!(error.to_string().contains("missing setup input 'initial'"), "{error}");
    }
}
