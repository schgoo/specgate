//! Internal bridge used by generated Cargo test-harness entry points.

use crate::{CaptureError, CaptureErrorKind, capture::Capture, registry, value::Value};
use std::any::Any;
use std::pin::Pin;

/// Type-erased synchronous constructor with explicit input failures.
pub type Constructor = fn(Vec<Value>) -> Result<Box<dyn Any>, CaptureError>;

/// Producer-local ownership-preserving adaptation to an unsized borrowed target.
pub type Projection = fn(Box<dyn Any>) -> Result<Box<dyn Any>, CaptureError>;

/// Lexically typed invocation whose future may borrow its erased receiver.
pub type Invocation =
    for<'a> fn(&'a mut dyn Any, Vec<Value>, Vec<Box<dyn Any>>) -> Pin<Box<dyn Future<Output = Result<(), CaptureError>> + 'a>>;

/// Erased setup constructor, emitted in the producer's own lexical scope.
#[derive(Debug, Clone, Copy)]
pub struct Setup {
    invoke: Constructor,
    output: fn() -> &'static str,
    dispatch: Option<Invocation>,
    projection: Option<Projection>,
}

impl Setup {
    /// Construct a generated setup bridge.
    #[must_use]
    pub const fn new(invoke: Constructor, output: fn() -> &'static str) -> Self {
        Self {
            invoke,
            output,
            dispatch: None,
            projection: None,
        }
    }

    /// Attach invocation code generated in the constructor's lexical scope.
    #[must_use]
    pub const fn dispatch(mut self, dispatch: Option<Invocation>) -> Self {
        self.dispatch = dispatch;
        self
    }

    /// Attach a projection that retains the producer's owned native value.
    #[must_use]
    pub const fn project(mut self, projection: Option<Projection>) -> Self {
        self.projection = projection;
        self
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
    let bridge = setup_bridge(component, operation, module, function, fills)?;
    let value = (bridge.invoke)(inputs)?;
    match bridge.projection {
        Some(project) => project(value),
        None => Ok(value),
    }
}

fn setup_bridge(component: &str, operation: &str, module: &str, function: &str, fills: &str) -> Result<Setup, CaptureError> {
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
    entry
        .replay_setup
        .ok_or_else(|| error(format!("setup '{module}::{function}' has no supported replay bridge")))
}

/// Select a setup-local invocation without exposing its concrete receiver type.
///
/// # Errors
/// Rejects missing or ambiguous setup declarations and missing invocation bridges.
pub fn setup_invocation(component: &str, operation: &str, module: &str, function: &str, fills: &str) -> Result<Invocation, CaptureError> {
    setup_bridge(component, operation, module, function, fills)?
        .dispatch
        .ok_or_else(|| error(format!("setup '{module}::{function}' has no selected operation bridge")))
}

/// Select an operation-local invocation, preserving private specialization paths.
///
/// # Errors
/// Rejects missing, ambiguous, or unsupported operation declarations.
pub fn operation_invocation(component: &str, operation: &str) -> Result<Invocation, CaptureError> {
    let mut matches = registry::OPERATIONS
        .iter()
        .filter(|entry| !entry.is_setup && entry.component == component && entry.name == operation);
    let entry = matches
        .next()
        .ok_or_else(|| error(format!("test target does not contain operation '{component}::{operation}'")))?;
    if matches.next().is_some() {
        return Err(error(format!("test target duplicates operation '{component}::{operation}'")));
    }
    entry
        .replay_operation
        .ok_or_else(|| error(format!("operation '{component}::{operation}' has no selected lexical bridge")))
}

/// Recover an injected value using the type inferred by its lexical invocation.
///
/// # Errors
/// Rejects a constructor result that does not match the selected native parameter.
pub fn take<T: Any>(value: Box<dyn Any>) -> Result<T, CaptureError> {
    value
        .downcast::<T>()
        .map(|value| *value)
        .map_err(|_value| error("selected setup returned a different native parameter type"))
}

/// Erase an owned producer behind its shared dereference target.
///
/// # Errors
/// Rejects a value that does not match the producer's native output type.
pub fn borrowed<T: std::ops::Deref + Any>(value: Box<dyn Any>) -> Result<Box<dyn Any>, CaptureError>
where
    T::Target: 'static,
{
    let owner: Box<dyn std::ops::Deref<Target = T::Target>> = Box::new(take::<T>(value)?);
    Ok(Box::new(owner))
}

/// Erase an owned producer behind its mutable dereference target.
///
/// # Errors
/// Rejects a value that does not match the producer's native output type.
pub fn borrowed_mut<T: std::ops::DerefMut + Any>(value: Box<dyn Any>) -> Result<Box<dyn Any>, CaptureError>
where
    T::Target: 'static,
{
    let owner: Box<dyn std::ops::DerefMut<Target = T::Target>> = Box::new(take::<T>(value)?);
    Ok(Box::new(owner))
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
input!(u32, Integer, u32::try_from);
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
            Value::Map(values) if values.is_empty() => Ok(()),
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

/// Complete the immediately ready bridges of a synchronous replay plan.
///
/// # Errors
/// Rejects a pending future; asynchronous plans must use the candidate's executor.
pub fn run_sync<F: Future>(future: F) -> Result<F::Output, CaptureError> {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
        std::task::Poll::Ready(output) => Ok(output),
        std::task::Poll::Pending => Err(error("synchronous replay unexpectedly suspended; configure a replay executor")),
    }
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
        <()>::decode(crate::ToNativeValue::to_native_value(&())).unwrap();
        <()>::decode(Value::Map([("unexpected".to_string(), Value::Integer(1))].into_iter().collect())).unwrap_err();
        <()>::decode(Value::List(Vec::new())).unwrap_err();
        assert_eq!(i32::decode(Value::Integer(7)).unwrap(), 7);
        assert_eq!(u32::decode(Value::Integer(i64::from(u32::MAX))).unwrap(), u32::MAX);
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

    #[test]
    fn synchronous_replay_does_not_supply_an_async_executor() {
        assert_eq!(super::run_sync(async { 7 }).unwrap(), 7);
        let error = super::run_sync(std::future::pending::<()>()).unwrap_err();
        assert!(error.to_string().contains("unexpectedly suspended"), "{error}");
    }

    #[test]
    fn producer_projections_preserve_owned_unsized_storage() {
        let value = super::borrowed::<Vec<i32>>(Box::new(vec![2, 3])).unwrap();
        let owner = super::take::<Box<dyn std::ops::Deref<Target = [i32]>>>(value).unwrap();
        assert_eq!(&**owner, &[2, 3]);

        let value = super::borrowed_mut::<String>(Box::new("owned".to_string())).unwrap();
        let mut owner = super::take::<Box<dyn std::ops::DerefMut<Target = str>>>(value).unwrap();
        (**owner).make_ascii_uppercase();
        assert_eq!(&**owner, "OWNED");
        super::borrowed::<String>(Box::new(7_i32)).unwrap_err();
    }
}
