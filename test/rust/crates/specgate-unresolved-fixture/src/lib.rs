//! Negative fixture for the "no dynamic / shapeless types" decision: an
//! operation whose `$result` is a plain struct that does NOT derive `SpecEvent`
//! (so it has no `TypeMeta`) and is not a primitive. Extraction cannot resolve
//! the type to a registered shape or a known primitive/collection, so it must
//! HARD-ERROR with an "unresolved type" reason rather than silently emitting a
//! placeholder.
use specgate::*;

spec_component!("fixture.unresolved");

/// A plain struct with no `#[derive(SpecEvent)]` — invisible to the registry.
pub struct Gadget {
    pub n: i32,
}

/// Operation returning the unregistered `Gadget` on its spec surface.
#[spec_operation("build")]
pub fn build() -> Gadget {
    Gadget { n: 7 }
}
