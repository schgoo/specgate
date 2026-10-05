//! Expansion for deterministic setup-producer annotations.
//!
//! The proc-macro facade calls [`expand_setup`] to validate setup declarations
//! and emit link-time registration plus capture metadata without changing the
//! annotated function's runtime behavior.

use super::{
    ItemFn, ReturnType, SetupArg, TokenStream2, collect_parameters, component, is_mut_ref, metadata_ident, parse_quote, quote, runtime,
};

/// Register a deterministic setup producer without modifying its behavior.
///
/// An async producer is registered but deliberately left uninstrumented:
/// capture state is thread-local and cannot follow a future across executor
/// threads. `specgate capture` therefore rejects any component that declares
/// an async setup, leaving it discovery-only.
///
/// # Examples
///
/// ```
/// use quote::quote;
/// let expanded = specgate_annotations_macros_impl::setup::expand_setup(
///     quote!("increment", spec = "example.counter"),
///     quote!(pub fn counter(start: i32) -> i32 { start }),
/// )?;
/// let expanded = expanded.to_string();
/// assert!(expanded.contains("OpMeta :: builder"));
/// assert!(expanded.contains("defer_setup"));
/// assert!(expanded.contains("example.counter"));
/// # Ok::<(), syn::Error>(())
/// ```
///
/// # Errors
/// Returns `syn::Error` when the macro input is malformed or uses an unsupported declaration.
pub fn expand_setup(attribute: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let SetupArg {
        operation,
        fills,
        component: owner,
    } = syn::parse2(attribute)?;
    let mut function: ItemFn = syn::parse2(item)?;
    let stacked = function.attrs.iter().any(|attribute| attribute.path().is_ident("spec_setup"));
    let params = collect_parameters(&mut function, !stacked);
    let rt = runtime();
    let component = component(owner.as_deref());
    let function_name = function.sig.ident.to_string();
    let const_name = metadata_ident("SETUP", format!("{function_name}:{operation}:{fills:?}"), function.sig.ident.span());
    let fills_metadata = fills.as_deref().map_or_else(
        || quote!(::core::option::Option::None),
        |fills| quote!(::core::option::Option::Some(#rt::FieldName::new(#fills))),
    );
    let is_async = function.sig.asyncness.is_some();
    if !is_async {
        let recorded = params
            .iter()
            .filter(|(_ident, ty, _name)| !is_mut_ref(ty))
            .map(|(ident, _ty, semantic_name)| quote!((#rt::FieldName::new(#semantic_name), #rt::ToNativeValue::to_native_value(&#ident))))
            .collect::<Vec<_>>();
        let body = function.block.clone();
        let return_type = match &function.sig.output {
            ReturnType::Default => quote!(()),
            ReturnType::Type(_, ty) => quote!(#ty),
        };
        *function.block = parse_quote!({
            let __sg_setup_inputs = #rt::defer_setup(
                #rt::SetupProvenance {
                    component_id: #rt::ComponentName::new(#component),
                    operation_name: #rt::OpName::new(#operation),
                    module_path: #rt::ModulePath::new(::core::module_path!()),
                    fn_name: #rt::FnName::new(#function_name),
                    fills: #fills_metadata,
                },
                || ::std::vec![#(#recorded),*],
            );
            let __sg_return = (move || -> #return_type #body)();
            if let ::std::result::Result::Err(error) = __sg_setup_inputs.commit() {
                #rt::report_error(#rt::Stage::SetupCommit, &error);
            }
            __sg_return
        });
    }
    let is_public = matches!(function.vis, syn::Visibility::Public(_));
    let parameter_metadata = params.iter().map(|(_ident, ty, name)| {
        let ty = quote!(#ty).to_string();
        quote!(#rt::FieldMeta::new(
            #rt::FieldName::new(#name),
            #rt::RustType::new(#ty),
        ))
    });
    let return_type = match &function.sig.output {
        ReturnType::Default => "()".to_string(),
        ReturnType::Type(_, ty) => quote!(#ty).to_string(),
    };
    let expanded = quote! {
        #function

        #[allow(dead_code, non_upper_case_globals, reason = "generated linker metadata is referenced externally")]
        const #const_name: () = {
            #[#rt::_private::linkme::distributed_slice(#rt::SPECGATE_OPS)]
            #[linkme(crate = #rt::_private::linkme)]
            static __SPECGATE_META: #rt::OpMeta = #rt::OpMeta::builder(#rt::OpMetaDeps {
                name: #rt::OpName::new(#operation),
                module_path: #rt::ModulePath::new(::core::module_path!()),
                fn_name: #rt::FnName::new(#function_name),
                params: &[#(#parameter_metadata),*],
                return_type: #rt::RustType::new(#return_type),
                component: #rt::ComponentName::new(#component),
            })
            .setup(#fills_metadata)
            .asynchronous(#is_async)
            .public(#is_public)
            .build();
        };
    };
    Ok(expanded)
}
