//! `SpecGate` runtime — the support library the annotation macros expand into.
//!
//! Provides native structured operation capture, semantic value projection,
//! and the link-time operation/type registry used by CTSC discovery.
//! Isolated test processes can activate native capture through
//! `SPECGATE_NATIVE_CAPTURE`; the first operation starts the session lazily and
//! each completed top-level operation atomically refreshes a stable JSON
//! sidecar containing the full scenario. Operation and setup finalizers consume
//! their guards and report failure explicitly. Guard destruction performs only
//! infallible in-memory cleanup; it never persists, panics, or writes stderr.
//! Manually started sessions remain active until `finish`.
//!
//! Captured inputs are the registry's black-box surface, not the raw call:
//! `#[spec_setup]` producers record their construction inputs, and the
//! operation they build adopts those inputs in place of the parameters the
//! setup fills. Attribution is by setup declaration, so running one declaration
//! twice in a capture is accepted only when both runs record value-identical
//! inputs; differing repeats are rejected rather than misattributed.
//!
//! Basic operation entry uses owned semantic identities:
//!
//! ```
//! use specgate_runtime::{ComponentId, OperationName, capture};
//!
//! let mut scope = capture::begin_operation(ComponentId::from("example"), OperationName::from("run"))?;
//! scope.unit()?;
//! # Ok::<(), specgate_runtime::CaptureError>(())
//! ```
//!
//! Companion to the `specgate-annotations-macros` proc-macro crate: the macros expand
//! into calls into this runtime, so user code never references it directly.
//! `ComponentId`, `OperationName`, and `TargetName` provide semantic
//! string identities without imposing lexical validation.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
#[cfg(any(test, feature = "test-util"))]
use std::sync::{Arc, Mutex};

// Stable CTSC fault vocabulary emitted when annotated target code unwinds.
const UNEXPECTED_FAULT: &str = "specgate.unexpected_target_fault";
/// Stable CTSC observer identifier for target-reported faults; changing it breaks trace-validator compatibility.
const TARGET_OBSERVER: &str = "target";
// OTLP trace IDs are exactly 16 bytes rendered as 32 lowercase hexadecimal digits.
const TRACE_HEX_LEN: usize = 32;
// OTLP span IDs are exactly 8 bytes rendered as 16 lowercase hexadecimal digits.
const SPAN_HEX_LEN: usize = 16;

/// Stable category for native-capture failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureErrorKind {
    /// Capture configuration or activation was invalid.
    Configuration,
    /// Capture persistence failed.
    Persistence,
    /// Repeated setup execution made construction attribution ambiguous.
    SetupAmbiguity,
    /// A required setup did not complete before its operation.
    SetupMissing,
}

/// Canonical native-capture failure with source chain and backtrace.
#[ohno::error]
#[display("{diagnostic}")]
pub struct CaptureError {
    kind: CaptureErrorKind,
    diagnostic: String,
}

struct SetupIdentity {
    component: registry::ComponentName,
    operation: registry::OpName,
    setup: registry::FnName,
}

struct SetupAmbiguity {
    identity: SetupIdentity,
    earlier: String,
    later: String,
}

impl CaptureError {
    fn message(kind: CaptureErrorKind, diagnostic: impl Into<String>) -> Self {
        Self::new(kind, diagnostic.into())
    }
    fn caused(kind: CaptureErrorKind, diagnostic: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(kind, diagnostic.into(), source)
    }
    fn setup_ambiguity(context: SetupAmbiguity) -> Self {
        let SetupAmbiguity {
            identity: SetupIdentity {
                component,
                operation,
                setup,
            },
            earlier,
            later,
        } = context;
        Self::message(
            CaptureErrorKind::SetupAmbiguity,
            format!(
                "setup '{setup}' for '{component}::{operation}' ran twice in one capture with different inputs \
                 ({earlier} then {later}); capture attributes construction inputs by declaration and cannot tell which instance a later \
                 '{operation}' invocation used. Capture one construction per scenario, or record identical inputs."
            ),
        )
    }
    fn setup_missing(component: impl AsRef<str>, operation: impl AsRef<str>, setup: impl AsRef<str>) -> Self {
        let component = component.as_ref();
        let operation = operation.as_ref();
        let setup = setup.as_ref();
        Self::message(
            CaptureErrorKind::SetupMissing,
            format!(
                "native capture for '{component}::{operation}' requires successful provenance from {setup}; invoke that #[spec_setup] \
                 during this capture and let it return successfully before calling '{operation}'"
            ),
        )
    }
    /// Stable failure category.
    #[must_use]
    pub const fn kind(&self) -> CaptureErrorKind {
        self.kind
    }
    /// Actionable failure diagnostic.
    #[must_use]
    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
    /// Whether the actionable diagnostic contains a substring.
    #[must_use]
    pub fn contains(&self, pattern: impl AsRef<str>) -> bool {
        self.diagnostic.contains(pattern.as_ref())
    }
}
impl PartialEq for CaptureError {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.diagnostic == other.diagnostic
    }
}
impl Eq for CaptureError {}
impl PartialEq<&str> for CaptureError {
    fn eq(&self, other: &&str) -> bool {
        self.diagnostic == *other
    }
}
impl AsRef<str> for CaptureError {
    fn as_ref(&self) -> &str {
        self.diagnostic()
    }
}
impl From<String> for CaptureError {
    fn from(diagnostic: String) -> Self {
        Self::message(CaptureErrorKind::Configuration, diagnostic)
    }
}

pub mod generated;

#[doc(hidden)]
pub mod __private {
    pub use linkme;
}

macro_rules! identity_string {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct an identity without applying lexical validation.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow the identity as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the identity and return its string representation.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.into_inner()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl ToNativeValue for $name {
            fn to_native_value(&self) -> Value {
                Value::String(self.0.clone())
            }
        }
    };
}

identity_string!(
    ComponentId,
    "A semantic CTSC component identifier.\n\nConstruct with `From`, borrow with `as_str` or `AsRef<str>`, and consume with `into_inner`."
);
identity_string!(
    OperationName,
    "A semantic CTSC operation name.\n\nConstruct with `From`, borrow with `as_str` or `AsRef<str>`, and consume with `into_inner`."
);
identity_string!(
    TargetName,
    "A binding-target selector name.\n\nConstruct with `From`, borrow with `as_str` or `AsRef<str>`, and consume with `into_inner`."
);

pub mod registry;

pub mod value;
use value::Value;

pub mod capture;

// ---------------------------------------------------------------------------
// Native observations and semantic value projection.
// ---------------------------------------------------------------------------

/// Record a typed observation on the currently active operation.
///
/// Outside a capture this is a no-op.
///
/// # Examples
/// ```
/// specgate_runtime::emit_event("attempt", &3_i32);
/// ```
///
/// # Panics
/// A panic from [`ToNativeValue::to_native_value`] propagates when capture is active.
pub fn emit_event<T: ToNativeValue + ?Sized>(name: impl Into<String>, value: &T) {
    emit_lazy(name, || value.to_native_value());
}

/// Record an observation while deferring semantic projection until capture is active.
///
/// Performance is guarded by `benches/native_capture.rs`: inactive entry and
/// event paths target less than 2 microseconds with no semantic projection or
/// allocation, while representative active completion targets 100 microseconds.
///
/// # Examples
/// ```
/// specgate_runtime::emit_lazy("attempt", || specgate_runtime::value::Value::Integer(3));
/// ```
///
/// # Panics
/// A panic from `project` propagates when capture is active.
pub fn emit_lazy(name: impl Into<String>, project: impl FnOnce() -> Value) {
    if !capture::is_active() {
        return;
    }
    let value = project();
    if let Err(error) = capture::record_observation(name.into(), value) {
        generated::report_error(generated::Stage::Observation, &error);
    }
}

/// Marker implemented by `#[derive(SpecEvent)]` for semantic record/union types.
pub trait SpecEvent: ToNativeValue {}

/// Convert a value to its native CTSC semantic representation.
///
/// # Examples
/// ```
/// use specgate_runtime::ToNativeValue;
/// use specgate_runtime::value::Value;
/// assert_eq!(3_i32.to_native_value(), Value::Integer(3));
/// ```
pub trait ToNativeValue {
    /// Project this value into its CTSC semantic representation.
    fn to_native_value(&self) -> Value;
}

macro_rules! native_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ToNativeValue for $ty {
                fn to_native_value(&self) -> Value {
                    Value::Integer(i64::from(*self))
                }
            }
        )*
    };
}

macro_rules! native_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ToNativeValue for $ty {
                fn to_native_value(&self) -> Value {
                    Value::Unsigned(u64::from(*self))
                }
            }
        )*
    };
}

native_signed!(i8, i16, i32);
native_signed!(u8, u16, u32);
native_unsigned!(u64);

impl ToNativeValue for i64 {
    fn to_native_value(&self) -> Value {
        Value::Integer(*self)
    }
}

impl ToNativeValue for isize {
    fn to_native_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToNativeValue for usize {
    fn to_native_value(&self) -> Value {
        Value::Unsigned(*self as u64)
    }
}

impl ToNativeValue for f32 {
    fn to_native_value(&self) -> Value {
        Value::Float(f64::from(*self))
    }
}

impl ToNativeValue for f64 {
    fn to_native_value(&self) -> Value {
        Value::Float(*self)
    }
}

impl ToNativeValue for bool {
    fn to_native_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl ToNativeValue for char {
    fn to_native_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToNativeValue for str {
    fn to_native_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToNativeValue for String {
    fn to_native_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl ToNativeValue for () {
    fn to_native_value(&self) -> Value {
        Value::Map(BTreeMap::new())
    }
}

impl<T: ToNativeValue> ToNativeValue for Vec<T> {
    fn to_native_value(&self) -> Value {
        Value::List(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue> ToNativeValue for [T] {
    fn to_native_value(&self) -> Value {
        Value::List(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue, const N: usize> ToNativeValue for [T; N] {
    fn to_native_value(&self) -> Value {
        self.as_slice().to_native_value()
    }
}

impl<T: ToNativeValue> ToNativeValue for BTreeMap<String, T> {
    fn to_native_value(&self) -> Value {
        Value::Map(self.iter().map(|(key, value)| (key.clone(), value.to_native_value())).collect())
    }
}

impl<T: ToNativeValue, S: std::hash::BuildHasher> ToNativeValue for HashMap<String, T, S> {
    fn to_native_value(&self) -> Value {
        Value::Map(self.iter().map(|(key, value)| (key.clone(), value.to_native_value())).collect())
    }
}

impl<T: ToNativeValue + Ord> ToNativeValue for BTreeSet<T> {
    fn to_native_value(&self) -> Value {
        Value::Set(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue + Eq + std::hash::Hash, S: std::hash::BuildHasher> ToNativeValue for HashSet<T, S> {
    fn to_native_value(&self) -> Value {
        let mut values = self.iter().map(ToNativeValue::to_native_value).collect::<Vec<_>>();
        values.sort();
        Value::Set(values.into_iter().collect())
    }
}

/// Compatibility-sensitive CTSC semantic map keys for Option and Result variants.
const OPTION_SOME_KEY: &str = "Some";
const OPTION_NONE_KEY: &str = "None";
const RESULT_OK_KEY: &str = "Ok";
const RESULT_ERR_KEY: &str = "Err";

impl<T: ToNativeValue> ToNativeValue for Option<T> {
    fn to_native_value(&self) -> Value {
        match self {
            Some(value) => Value::Map(BTreeMap::from([(OPTION_SOME_KEY.to_string(), value.to_native_value())])),
            None => Value::Map(BTreeMap::from([(OPTION_NONE_KEY.to_string(), Value::Map(BTreeMap::new()))])),
        }
    }
}

impl<T: ToNativeValue, E: ToNativeValue> ToNativeValue for Result<T, E> {
    fn to_native_value(&self) -> Value {
        match self {
            Ok(value) => Value::Map(BTreeMap::from([(RESULT_OK_KEY.to_string(), value.to_native_value())])),
            Err(error) => Value::Map(BTreeMap::from([(RESULT_ERR_KEY.to_string(), error.to_native_value())])),
        }
    }
}

impl ToNativeValue for Value {
    fn to_native_value(&self) -> Value {
        self.clone()
    }
}

impl<T: ToNativeValue + ?Sized> ToNativeValue for &T {
    fn to_native_value(&self) -> Value {
        (**self).to_native_value()
    }
}

impl<T: ToNativeValue + ?Sized> ToNativeValue for Box<T> {
    fn to_native_value(&self) -> Value {
        (**self).to_native_value()
    }
}

macro_rules! native_tuple {
    ($($name:ident),+ $(,)?) => {
        impl<$($name: ToNativeValue),+> ToNativeValue for ($($name,)+) {
            #[expect(non_snake_case, reason = "tuple bindings intentionally match their generic type parameters")]
            fn to_native_value(&self) -> Value {
                let ($($name,)+) = self;
                Value::List(vec![$($name.to_native_value()),+])
            }
        }
    };
}

native_tuple!(A, B);
native_tuple!(A, B, C);
native_tuple!(A, B, C, D);
