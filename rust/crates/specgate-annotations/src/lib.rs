//! `SpecGate` annotations — the public façade that annotated code depends on.
//!
//! Re-exports the proc-macros from `specgate-annotations-macros`
//! (`#[spec_operation]`, `#[spec_setup]`, `#[spec_mock]`,
//! `#[derive(SpecEvent)]`, `#[spec_event]`, `#[spec_input]`,
//! `spec_component!`, `spec_trace!`) and the runtime support from
//! `specgate-runtime`. Annotated code typically does
//! `use specgate_annotations::*;` (or `use specgate::*;` via the umbrella
//! crate) to pull in everything at once.
//!
//! Synchronous operation annotations also provide native structured invocation
//! boundaries to the runtime when a capture session is active. Existing flat
//! trace emission remains available as the compatibility view.
//!
//! Annotations are zero-cost in production: without the trace feature the
//! macros expand to no-ops.

pub use specgate_annotations_macros::{SpecEvent, spec_component, spec_mock, spec_operation, spec_setup, spec_trace};
// Re-export the SpecEvent trait under the same name — traits live in the
// type namespace while the derive macro lives in the macro namespace, so
// they coexist without conflict.
pub use specgate_runtime::{SpecEvent, ToSpecValue, TraceEvent, Value, take_traces};

#[doc(hidden)]
pub mod __rt {
    pub use specgate_runtime::linkme;
    pub use specgate_runtime::{
        NativeCapture, NativeCaptureConfig, NativeCompletion, NativeObservation, NativeOperationSpan, NativeSpanBoundary, NativeStatus,
        OpMeta, OperationScope, ReturnEmit, ReturnEmitDisplay, ReturnEmitNone, ReturnEmitStruct, ReturnEmitToSpec, SPECGATE_OPS,
        SPECGATE_TYPES, SpecEvent, SpecEventStruct, ToSpecValue, TraceEvent, TypeMeta, Value, VariantMeta, begin_native_operation,
        discovery_json, emit_event, emit_event_v, emit_input_event_v, emit_result_event_v, emit_run, finish_native_capture, mock_lookup,
        record_event_only, reset, set_mock, start_native_capture, take_traces,
    };
}

// Re-export auxiliary runtime helpers under their plain names too.
pub use specgate_runtime::{emit_event_v, emit_run, mock_lookup, record_event_only, reset, set_mock};
