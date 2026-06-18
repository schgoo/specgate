//! SpecGate annotations — public façade.
//!
//! Re-exports the proc-macros from `specgate-annotations-macros` and the
//! runtime support from `specgate-runtime`. Fixture code typically does
//! `use specgate_annotations::*;` to pull in everything in one shot.

pub use specgate_annotations_macros::{
    spec_mock, spec_operation, spec_setup, spec_trace, SpecEvent,
};
// Re-export the SpecEvent trait under the same name — traits live in the
// type namespace while the derive macro lives in the macro namespace, so
// they coexist without conflict.
pub use specgate_runtime::{take_traces, SpecEvent, TraceEvent};

#[doc(hidden)]
pub mod __rt {
    pub use specgate_runtime::*;
}

// Re-export auxiliary runtime helpers under their plain names too.
pub use specgate_runtime::{emit_event, emit_run, mock_lookup, reset, set_mock};
