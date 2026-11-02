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
/// assert!(expanded.contains("OpMeta :: const_builder"));
/// assert!(expanded.contains("defer_setup"));
/// assert!(expanded.contains("example.counter"));
/// # Ok::<(), syn::Error>(())
/// ```
///
/// # Errors
/// Returns `syn::Error` when the macro input is malformed or uses an unsupported declaration.
///
/// # Panics
/// Panics if primitive setup-input validation and bridge generation disagree.
pub fn expand_setup(attribute: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let SetupArg {
        operation,
        fills,
        component: owner,
    } = syn::parse2(attribute)?;
    let mut function: ItemFn = syn::parse2(item)?;
    if let Some(constness) = &function.sig.constness {
        return Err(syn::Error::new_spanned(
            constness,
            "`#[spec_setup]` does not support `const fn`: capture needs runtime state that const evaluation \
             cannot use; annotate a non-const wrapper that calls this function",
        ));
    }
    let stacked = function.attrs.iter().any(|attribute| attribute.path().is_ident("spec_setup"));
    let params = collect_parameters(&mut function, !stacked);
    let rt = runtime();
    let component = component(owner.as_deref());
    let explicit_component = owner.is_some();
    let function_name = function.sig.ident.to_string();
    let const_name = metadata_ident("SETUP", format!("{function_name}:{operation}:{fills:?}"), function.sig.ident.span());
    let fills_metadata = fills.as_deref().map_or_else(
        || quote!(::core::option::Option::None),
        |fills| quote!(::core::option::Option::Some(#rt::FieldName::new(#fills))),
    );
    let is_async = function.sig.asyncness.is_some();
    let is_method = super::has_receiver(&function);
    if !is_async {
        let recorded = params
            .iter()
            .map(|(ident, ty, semantic_name)| {
                let value = if is_mut_ref(ty) { quote!(&*#ident) } else { quote!(&#ident) };
                quote!((#rt::FieldName::new(#semantic_name), #rt::ToNativeValue::to_native_value(#value)))
            })
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
    let replay = if std::env::var_os("SPECGATE_TEST_ENTRY").is_some()
        && !is_async
        && function.sig.generics.params.is_empty()
        && !super::has_receiver(&function)
        && owned_output(&function.sig.output)
    {
        let function_ident = &function.sig.ident;
        let construct = if params.iter().all(|(_, ty, _)| primitive(ty).is_some()) {
            let bindings = params.iter().map(|(ident, ty, name)| {
                let (ty, _) = primitive(ty).expect("validated primitive setup input");
                quote!(let #ident: #ty = #rt::harness::argument(&mut values, #name)?;)
            });
            let arguments = params.iter().map(|(ident, ty, _)| {
                if primitive(ty).expect("validated primitive setup input").1 {
                    quote!(&#ident)
                } else {
                    quote!(#ident)
                }
            });
            quote! {
                let mut values = values.into_iter();
                #(#bindings)*
                if values.next().is_some() {
                    return ::core::result::Result::Err(#rt::harness::error("unexpected extra setup inputs"));
                }
                ::core::result::Result::Ok(::std::boxed::Box::new(#function_ident(#(#arguments),*)))
            }
        } else if std::env::var_os("SPECGATE_TEST_DISPATCH").is_some() {
            let declared_fills = fills.as_deref().unwrap_or_default();
            let package = std::env::var("CARGO_PKG_NAME").map_err(|error| syn::Error::new_spanned(&function.sig, error))?;
            quote! {
                let constructor: ::core::option::Option<#rt::harness::Constructor> = {
                    use #rt as __specgate;
                    macro_rules! __specgate_selected {
                        () => {
                            __specgate_dispatch!(construction, #package, #component, #operation, #function_name, #declared_fills, #function_ident)
                        };
                    }
                    include!(env!(concat!("SPECGATE_TEST_MODULE_", module_path!())))
                };
                constructor.ok_or_else(|| #rt::harness::error("setup requires a selected replay constructor"))?(values)
            }
        } else {
            quote! {
                let _ = values;
                ::core::result::Result::Err(#rt::harness::error("setup requires a selected replay constructor"))
            }
        };
        let output = match &function.sig.output {
            ReturnType::Default => quote!(()),
            ReturnType::Type(_, ty) => quote!(#ty),
        };
        let dispatch = if std::env::var_os("SPECGATE_TEST_DISPATCH").is_some() {
            let declared_fills = fills.as_deref().unwrap_or_default();
            let package = std::env::var("CARGO_PKG_NAME").map_err(|error| syn::Error::new_spanned(&function.sig, error))?;
            quote! {
                .dispatch({
                    use #rt as __specgate;
                    macro_rules! __specgate_selected {
                        () => { __specgate_dispatch!(#package, setup, #component, #operation, #function_name, #declared_fills, #output) };
                    }
                    include!(env!(concat!("SPECGATE_TEST_MODULE_", module_path!())))
                })
                .project({
                    use #rt as __specgate;
                    macro_rules! __specgate_selected {
                        () => { __specgate_dispatch!(input, #package, #component, #operation, #function_name, #declared_fills, #output) };
                    }
                    include!(env!(concat!("SPECGATE_TEST_MODULE_", module_path!())))
                })
            }
        } else {
            TokenStream2::new()
        };
        quote! {
            .replay_setup(#rt::harness::Setup::new(
                |values| {
                    #construct
                },
                || ::core::any::type_name::<#output>(),
            ) #dispatch)
        }
    } else {
        TokenStream2::new()
    };
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
    let dependency = super::harness::require_component();
    let expanded = quote! {
        #function

        #[allow(dead_code, non_upper_case_globals, reason = "generated linker metadata is referenced externally")]
        const #const_name: () = {
            #dependency
            #[#rt::_private::linkme::distributed_slice(#rt::SPECGATE_OPS)]
            #[linkme(crate = #rt::_private::linkme)]
            static __SPECGATE_META: #rt::OpMeta = #rt::OpMeta::const_builder(#rt::OpDeps {
                name: #rt::OpName::new(#operation),
                module_path: #rt::ModulePath::new(::core::module_path!()),
                fn_name: #rt::FnName::new(#function_name),
                params: &[#(#parameter_metadata),*],
                return_type: #rt::RustType::new(#return_type),
                component: #rt::ComponentName::new(#component),
            })
            .setup(#fills_metadata)
            .method(#is_method)
            .explicit_component(#explicit_component)
            .asynchronous(#is_async)
            .public(#is_public)
            #replay
            .build();
        };
    };
    Ok(expanded)
}

fn primitive(ty: &syn::Type) -> Option<(syn::Type, bool)> {
    let text = quote!(#ty).to_string().replace(' ', "");
    match text.as_str() {
        "&str" => Some((parse_quote!(String), true)),
        "String" | "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" | "()" => Some((ty.clone(), false)),
        _ => {
            let syn::Type::Path(path) = ty else {
                return None;
            };
            let segment = path.path.segments.last()?;
            let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return None;
            };
            let count = match segment.ident.to_string().as_str() {
                "Vec" | "Option" | "BTreeSet" | "HashSet" => 1,
                "BTreeMap" | "HashMap" => 2,
                _ => return None,
            };
            (arguments.args.len() == count
                && arguments.args.iter().all(
                    |argument| matches!(argument, syn::GenericArgument::Type(ty) if primitive(ty).is_some_and(|(_, borrowed)| !borrowed)),
                ))
            .then(|| (ty.clone(), false))
        }
    }
}

fn owned_output(output: &ReturnType) -> bool {
    struct Owned(bool);
    impl syn::visit_mut::VisitMut for Owned {
        fn visit_type_mut(&mut self, i: &mut syn::Type) {
            if matches!(
                i,
                syn::Type::Reference(_) | syn::Type::ImplTrait(_) | syn::Type::Infer(_) | syn::Type::Macro(_)
            ) {
                self.0 = false;
            }
            syn::visit_mut::visit_type_mut(self, i);
        }

        fn visit_lifetime_mut(&mut self, i: &mut syn::Lifetime) {
            if i.ident != "static" {
                self.0 = false;
            }
        }
    }
    let mut output = output.clone();
    let mut owned = Owned(true);
    syn::visit_mut::VisitMut::visit_return_type_mut(&mut owned, &mut output);
    owned.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn const_fn_setups_are_rejected() {
        let error = super::expand_setup(
            quote::quote!("read"),
            quote::quote!(
                pub const fn store(capacity: usize) -> usize {
                    capacity
                }
            ),
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not support `const fn`"), "{error}");
        assert!(error.to_string().contains("non-const wrapper"), "{error}");
    }
}
