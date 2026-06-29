//! Two-component fixture in ONE crate, used to test the component axis of
//! `specgate extract`: cross-component `depends_on` derivation and multi-component
//! selection. The crate-root `spec_component!` sets the DEFAULT component
//! (`comp.core`); a per-item `spec = "…"` override puts `assemble` in a second
//! component (`comp.app`).
//!
//! Layout:
//!   * `comp.core` (default)  — owns the `Widget` SpecEvent type and the
//!     `make_widget` operation (returns `Widget`, a local type → no depends_on).
//!   * `comp.app`  (override) — owns `assemble`, whose `$result` is `Widget`.
//!     Because `Widget` belongs to `comp.core`, extracting `comp.app` derives
//!     `depends_on: [comp.core]` and references `Widget` by bare name without
//!     redefining it.
//!
//! Extracting this crate WITHOUT a `--component` selector is ambiguous (two
//! components present) and must error.
use specgate::*;

spec_component!("comp.core");

/// A `SpecEvent` type owned by `comp.core` (crate-root default component).
#[derive(SpecEvent)]
pub struct Widget {
    #[spec_event]
    pub id: i32,
    #[spec_event]
    pub label: String,
}

/// Operation owned by `comp.core`: returns a local `Widget` (no depends_on).
#[spec_operation("make_widget")]
pub fn make_widget() -> Widget {
    Widget {
        id: 1,
        label: "widget".to_string(),
    }
}

/// Operation owned by `comp.app` via a per-item override. Its `$result` is a
/// `Widget` owned by `comp.core`, exercising cross-component `depends_on`.
#[spec_operation("assemble", spec = "comp.app")]
pub fn assemble() -> Widget {
    Widget {
        id: 2,
        label: "assembled".to_string(),
    }
}
