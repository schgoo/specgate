//! Generated entry points are enabled only for CLI-owned test builds.

use super::{TokenStream2, metadata_ident, quote, runtime};

/// Emit a libtest entry point in the annotated item's lexical module.
///
/// The unconditional `option_env!` tracks entry identity even in ordinary
/// builds, without overriding the candidate's Cargo-configured compiler flags.
#[must_use]
pub fn entry(identity: &str) -> TokenStream2 {
    let dependency = quote!(
        const _: ::core::option::Option<&str> = ::core::option_env!("SPECGATE_TEST_ENTRY");
        const _: ::core::option::Option<&str> = ::core::option_env!("SPECGATE_TEST_DISPATCH");
    );
    if std::env::var_os("SPECGATE_TEST_ENTRY").is_none() {
        return dependency;
    }
    let rt = runtime();
    let name = metadata_ident("harness", identity, proc_macro2::Span::call_site());
    quote! {
        #dependency
        #[cfg(test)]
        #[test]
        #[allow(non_snake_case, reason = "generated libtest bridge has a collision-resistant internal name")]
        fn #name() {
            use #rt as __specgate;
            include!(env!("SPECGATE_TEST_ENTRY"));
        }
    }
}
