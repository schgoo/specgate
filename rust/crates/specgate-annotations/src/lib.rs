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
        OpMeta, ReturnEmit, ReturnEmitDisplay, ReturnEmitNone, ReturnEmitStruct, ReturnEmitToSpec, SPECGATE_OPS, SPECGATE_TYPES, SpecEvent,
        SpecEventStruct, ToSpecValue, TraceEvent, TypeMeta, Value, VariantMeta, discovery_json, emit_event, emit_event_v, emit_run,
        mock_lookup, reset, set_mock, take_traces,
    };
}

// Re-export auxiliary runtime helpers under their plain names too.
pub use specgate_runtime::{emit_event, emit_event_v, emit_run, mock_lookup, reset, set_mock};
