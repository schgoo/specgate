//! Annotation macros that connect native Rust code to the `SpecGate` CTSC runtime.
//!
//! [`spec_operation`] marks functions and methods as captured operation boundaries,
//! while [`spec_setup`] marks deterministic producers used to construct operation
//! inputs or receivers. Both attributes preserve the annotated item's behavior and
//! emit hygienic link-time metadata consumed by discovery. Components are selected
//! with the facade's `spec_component!` macro; event projection is provided by the
//! facade's `SpecEvent` derive and `spec_trace!` macro.
//!
//! Most applications import these established names from `specgate`; this
//! implementation-facing crate exists so the facade can re-export the attributes
//! without changing generated runtime identity.

use proc_macro::TokenStream;

fn finish(result: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Mark a function or method as a native CTSC operation boundary.
///
/// The first argument is the registry operation name; optional `spec = "component"`
/// overrides the surrounding [`specgate::spec_component!`] declaration. Inputs and
/// return values must implement `SpecGate` native projection. Malformed attributes,
/// unsupported patterns, and unsupported return shapes are compile-time errors.
///
/// # Examples
///
/// ```
/// #[specgate::spec_operation("add", spec = "example.math")]
/// fn add(left: i32, right: i32) -> i32 { left + right }
/// ```
#[proc_macro_attribute]
pub fn spec_operation(attribute: TokenStream, item: TokenStream) -> TokenStream {
    finish(specgate_annotations_macros_impl::operation::expand_operation(
        attribute.into(),
        item.into(),
    ))
}

/// Register a deterministic setup producer without modifying its behavior.
///
/// The operation name is required; `fills = "parameter"` and `spec = "component"`
/// select the filled input and owner. Unsupported declarations are compile-time errors.
///
/// # Examples
///
/// ```
/// #[specgate::spec_setup("add", fills = "left", spec = "example.math")]
/// fn default_left() -> i32 { 1 }
/// ```
#[proc_macro_attribute]
pub fn spec_setup(attribute: TokenStream, item: TokenStream) -> TokenStream {
    finish(specgate_annotations_macros_impl::setup::expand_setup(attribute.into(), item.into()))
}

/// Derive native semantic projection and link-time type metadata.
///
/// Fields may use `#[spec_event(name = "wire_name")]` or `#[spec_event(path)]`.
/// Invalid attributes and unsupported data shapes are compile-time errors.
///
/// # Examples
///
/// ```
/// #[derive(specgate::SpecEvent)]
/// #[spec_component("example.point")]
/// struct Point { x: i32, y: i32 }
/// ```
#[proc_macro_derive(SpecEvent, attributes(spec_event, spec_component))]
pub fn spec_event(input: TokenStream) -> TokenStream {
    finish(specgate_annotations_macros_impl::event::expand_event(input.into()))
}
