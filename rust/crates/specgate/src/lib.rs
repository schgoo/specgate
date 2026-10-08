//! Umbrella crate for `SpecGate`'s native CTSC annotation surface.
//!
//! Add `specgate` to an implementation crate, declare a component, annotate
//! operations/setups/types, and exercise behavior through ordinary tests.
//! `specgate capture` records those real invocations as deterministic CTSC
//! reference traces; `specgate replay` invokes a candidate from the captured
//! semantic inputs. Async operations open at first poll and preserve their construction parent across executor-thread migration and same-thread interleaving. Async setups remain unsupported. `ComponentId`,
//! `OperationName`, and `TargetName` distinguish semantic identities while
//! retaining transparent CTSC string projection and unrestricted string input.
//!
//! ```rust
//! use specgate::{spec_component, spec_operation};
//!
//! spec_component!("example.math");
//!
//! #[spec_operation("add")]
//! pub fn add(a: i32, b: i32) -> i32 {
//!     add_impl(a, b)
//! }
//!
//! fn add_impl(a: i32, b: i32) -> i32 {
//!     a + b
//! }
//!
//! fn main() {}
//! ```

#[expect(
    unused_extern_crates,
    reason = "proc-macro expansions resolve the facade crate through this self alias"
)]
extern crate self as specgate;

pub use specgate_annotations_macros::{SpecEvent, spec_operation, spec_setup};
pub use specgate_runtime::value::Value;
pub use specgate_runtime::{ComponentId, OperationName, SpecEvent, TargetName, ToNativeValue};

/// Declare the crate's default component identifier.
#[macro_export]
macro_rules! spec_component {
    ($component:literal $(,)?) => {
        #[doc(hidden)]
        const __SPECGATE_COMPONENT: &str = $component;
    };
}

/// Record one lazily projected native observation in the active operation.
///
/// The first argument supplies the semantic event name. The second is borrowed
/// and converted through [`ToNativeValue`](crate::ToNativeValue) only while a
/// capture context is active, so side-effect-free projection has zero inactive
/// work beyond the context check. Outside an operation the event is ignored.
///
/// # Examples
///
/// ```
/// # use specgate::{spec_trace, Value};
/// spec_trace!("cache.hit", true);
/// ```
#[macro_export]
macro_rules! spec_trace {
    ($name:expr, $value:expr $(,)?) => {
        $crate::__rt::emit_lazy($name, || $crate::__rt::ToNativeValue::to_native_value(&$value));
    };
}

#[doc(hidden)]
pub mod __rt {
    #[doc(hidden)]
    pub mod _private {
        pub use specgate_runtime::__private::linkme;
    }
    pub use specgate_runtime::capture::{
        Capture, Completion, Config, ConfigDeps, EnvConfig, Observation, OperationScope, OperationSpan, SetupProvenance, SpanBoundary,
        SpanId, Status, TraceId, begin_async_operation, begin_operation, capture_async_context, defer_setup, finish,
        instrument_async_operation, record_setup, start,
    };
    pub use specgate_runtime::generated::{FAILURE_MARKER, Stage, report_completion, report_error};
    pub use specgate_runtime::registry::{
        ComponentName, FieldMeta, FieldName, FnName, ModulePath, OPERATIONS as SPECGATE_OPS, OpDeps, OpMeta, OpName, RustType,
        TYPES as SPECGATE_TYPES, TypeDeps, TypeKind, TypeMeta, TypeName, VariantMeta, VariantName, discovery,
    };
    pub use specgate_runtime::value::Value;
    pub use specgate_runtime::{ComponentId, OperationName, SpecEvent, TargetName, ToNativeValue, emit_event, emit_lazy};
}
