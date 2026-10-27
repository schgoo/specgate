//! Generated entry points are enabled only for CLI-owned test builds.

use super::{TokenStream2, metadata_ident, quote, runtime};

/// Track environment-dependent annotation expansion in every Cargo build.
#[must_use]
pub fn dependency() -> TokenStream2 {
    quote!(
        const _: ::core::option::Option<&str> = ::core::option_env!("SPECGATE_TEST_ENTRY");
        const _: ::core::option::Option<&str> = ::core::option_env!("SPECGATE_TEST_DISPATCH");
    )
}

/// Require a target-local component only in generated test builds.
#[must_use]
pub fn require_component() -> TokenStream2 {
    let dependency = dependency();
    let marker = std::env::var_os("SPECGATE_TEST_ENTRY").is_some().then(|| {
        quote! {
            #[cfg(test)]
            const _: () = crate::__specgate_requires_crate_root_spec_component;
        }
    });
    quote! {
        #dependency
        #marker
    }
}

/// Emit a libtest entry point at the root declaring `spec_component!`.
#[must_use]
pub fn entry(identity: &str) -> TokenStream2 {
    let dependency = dependency();
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
