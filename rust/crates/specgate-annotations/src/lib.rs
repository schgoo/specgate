//! SpecGate annotations.
//!
//! This crate exists primarily as the documented home of the
//! `#[spec_setup]`, `#[spec_operation]`, `#[spec_event]`, `#[spec_mock]`,
//! and `spec_event!` annotation surface. The current SpecGate harness
//! does not require these annotations to be present in compiled form —
//! it consumes annotated source by **reading and interpreting** the
//! source files directly (see `specgate-harness`). The proc-macros below
//! therefore expand to no-ops, which is enough for source files that
//! `use specgate_annotations::*` to parse without symbol-resolution
//! errors when an external tool feeds them to `syn`.
//!
//! Note that `#[spec_event]` placed on a bare struct field is rejected
//! by `rustc` (procedural attribute macros are not allowed on field
//! positions without a containing macro). The harness sidesteps this by
//! never feeding fixture sources to `rustc`. They are only ever parsed
//! with `syn` and interpreted symbolically.

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn spec_operation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn spec_setup(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn spec_event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn spec_mock(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro]
pub fn spec_event_record(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
