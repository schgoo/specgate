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
//!
//! Generic operation methods declare concrete specializations explicitly with
//! `#[spec_operation("find_items", types = "crate::Item")]`. Repeat the attribute
//! with distinct semantic names for additional type combinations; multiple type
//! arguments use a comma-separated string. Generic parameters keep their authored
//! bounds and require `'static` for specialization dispatch. Setup construction
//! inputs support primitives and recursive collections and optionals;
//! `#[spec_event(setup)]` opts a `SpecEvent` type into property-setup folding.
//! Synchronous annotated test-only producers can rebuild private replay
//! environments through generated Cargo test entry points.

use proc_macro::TokenStream;

fn finish(result: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}

#[doc(hidden)]
#[proc_macro]
pub fn __specgate_test_harness(_input: TokenStream) -> TokenStream {
    specgate_annotations_macros_impl::harness::entry("root").into()
}

/// Mark a function or method as a native CTSC operation boundary.
///
/// The first argument is the registry operation name; optional `spec = "component"`
/// overrides the surrounding [`specgate::spec_component!`] declaration. Inputs and
/// return values must implement `SpecGate` native projection. Malformed attributes,
/// unsupported patterns, and unsupported return shapes are compile-time errors.
/// `#[spec_input(ignore)]` (or `#[spec_input("name", ignore)]`) excludes a
/// parameter from semantic inputs. Its type needs only `Default`, not native
/// projection; replay constructs that default in the original argument position.
/// Ordinary calls, including capture, retain the supplied value without defaulting.
///
/// Ignored parameters must implement `Default`:
///
/// ```compile_fail
/// struct Context;
/// #[specgate::spec_operation("run", spec = "example.context")]
/// fn run(#[spec_input(ignore)] context: Context) { let _ = context; }
/// ```
///
/// # Examples
///
/// ```
/// #[specgate::spec_operation("add", spec = "example.math")]
/// fn add(left: i32, right: i32) -> i32 { left + right }
/// ```
///
/// A `const fn` is rejected because capture needs runtime state:
///
/// ```compile_fail
/// #[specgate::spec_operation("add", spec = "example.math")]
/// const fn add(left: i32, right: i32) -> i32 { left + right }
/// ```
///
/// Annotate a non-const wrapper instead, and call the wrapper from tests:
///
/// ```
/// const fn add(left: i32, right: i32) -> i32 { left + right }
///
/// #[specgate::spec_operation("add", spec = "example.math")]
/// fn checked_add(left: i32, right: i32) -> i32 { add(left, right) }
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
/// select the filled input and owner. Unsupported declarations, including `const fn`,
/// are compile-time errors.
/// Synchronous producer parameters support the same `#[spec_input(ignore)]`
/// and compile-time `Default` requirement as [`spec_operation`].
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
///
/// Fields marked `#[spec_event(ignore)]` are absent from projection and type
/// metadata, including enum payload fields. Ignored tuple positions are compacted.
/// For `#[spec_event(setup)]`, replay decodes selected fields first, then uses
/// the whole struct's `Default` for omissions. Only explicitly ignored omissions
/// may fall back to their field types' `Default` when the struct has none.
/// Producers own their output; projection alone does not enable reconstruction.
///
/// Neither default source is available here:
///
/// ```compile_fail
/// specgate::spec_component!("example.no_default");
/// struct Opaque;
/// #[derive(specgate::SpecEvent)]
/// #[spec_event(setup)]
/// struct Inputs {
///     #[spec_event] pub value: i32,
///     #[spec_event(ignore)] context: Opaque,
/// }
/// ```
///
/// Unannotated omissions still require a whole-struct default, even if each
/// omitted field has a default:
///
/// ```compile_fail
/// specgate::spec_component!("example.unannotated");
/// #[derive(specgate::SpecEvent)]
/// #[spec_event(setup)]
/// struct Inputs {
///     #[spec_event] pub value: i32,
///     #[spec_event(ignore)] context: i32,
///     internal: i32,
/// }
/// ```
#[proc_macro_derive(SpecEvent, attributes(spec_event, spec_component))]
pub fn spec_event(input: TokenStream) -> TokenStream {
    finish(specgate_annotations_macros_impl::event::expand_event(input.into()))
}
