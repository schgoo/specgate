//! Umbrella crate for `SpecGate`'s native CTSC annotation surface.
//!
//! Add `specgate` to an implementation crate, declare a component, annotate
//! operations/setups/types, and exercise behavior through ordinary tests.
//! `specgate capture` records those real invocations as deterministic CTSC
//! reference traces; `specgate replay` invokes a candidate from the captured
//! semantic inputs. Async operations remain discoverable but are not natively
//! captured until capture context becomes task-safe.
//!
//! ```rust
//! use specgate::*;
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

#[allow(unused_extern_crates)]
extern crate self as specgate;

pub use specgate_annotations_macros::{SpecEvent, spec_component, spec_operation, spec_setup, spec_trace};
pub use specgate_runtime::{SpecEvent, ToNativeValue, Value};

#[doc(hidden)]
pub mod __rt {
    pub use specgate_runtime::linkme;
    pub use specgate_runtime::{
        NativeCapture, NativeCaptureConfig, NativeCaptureEnvironmentConfig, NativeCompletion, NativeObservation, NativeOperationSpan,
        NativeSpanBoundary, NativeStatus, OpMeta, OperationScope, SPECGATE_OPS, SPECGATE_TYPES, SpecEvent, ToNativeValue, TypeMeta, Value,
        VariantMeta, begin_native_operation, discovery_json, emit_event, finish_native_capture, reject_async_native_capture,
        start_native_capture,
    };
}
