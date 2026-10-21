//! `spec_operation` attribute expansion.
//!
//! [`expand_operation`] accepts attribute tokens and one function or method,
//! preserving the declaration while adding capture-boundary instrumentation
//! and link-time metadata. Use it through the proc-macro facade unless testing
//! expansion behavior directly.

use super::{
    ERROR_NAME, ItemFn, OperationArg, ReturnKind, ReturnType, TokenStream2, Type, component, has_receiver, is_mut_ref, metadata_ident,
    parameters, parse_quote, quote, return_kind, runtime,
};

/// Stable namespace used to derive hidden operation metadata symbol names.
///
/// Changing this discriminator renames link-time registration symbols and can
/// cause collisions with metadata emitted by other annotation kinds.
const METADATA_NAMESPACE: &str = "OPERATION";

/// Build the scope-opening fragment for an operation body.
///
/// `abandonable` selects the async entry point, which permits a dropped,
/// never-resolved future to record abandonment instead of poisoning the
/// session. A synchronous body cannot stop part-way without unwinding, so it
/// keeps the strict entry point.
fn begin_fragment(rt: &TokenStream2, component: &TokenStream2, name: &TokenStream2, abandonable: bool) -> TokenStream2 {
    let begin = if abandonable {
        quote!(begin_async_operation)
    } else {
        quote!(begin_operation)
    };
    quote! {
        let mut __sg_scope = match match #name {
            ::core::option::Option::Some(name) => #rt::#begin(
                #rt::ComponentId::from(#component),
                #rt::OperationName::from(name),
            ),
            ::core::option::Option::None => #rt::reject_undeclared_specialization(),
        } {
            ::std::result::Result::Ok(scope) => scope,
            ::std::result::Result::Err(error) => {
                #rt::report_error(#rt::Stage::OperationBegin, &error);
                #rt::OperationScope::inactive()
            }
        };
    }
}

fn async_completion(rt: &TokenStream2, kind: ReturnKind) -> TokenStream2 {
    match kind {
        ReturnKind::Unit => quote!(#rt::report_completion(__sg_scope.unit());),
        ReturnKind::Option => quote! {
            match &__sg_return {
                ::std::option::Option::Some(value) => #rt::report_completion(__sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(value))),
                ::std::option::Option::None => #rt::report_completion(__sg_scope.empty()),
            }
        },
        ReturnKind::OptionUnit | ReturnKind::Value => quote! {
            #rt::report_completion(__sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(&__sg_return)));
        },
        ReturnKind::Result => quote! {
            match &__sg_return {
                ::std::result::Result::Ok(value) => #rt::report_completion(__sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(value))),
                ::std::result::Result::Err(error) => #rt::report_completion(__sg_scope.error_lazy(#ERROR_NAME, || #rt::ToNativeValue::to_native_value(error))),
            }
        },
        ReturnKind::ResultUnit => quote! {
            match &__sg_return {
                ::std::result::Result::Ok(()) => #rt::report_completion(__sg_scope.unit()),
                ::std::result::Result::Err(error) => #rt::report_completion(__sg_scope.error_lazy(#ERROR_NAME, || #rt::ToNativeValue::to_native_value(error))),
            }
        },
        ReturnKind::ResultErrUnit => quote! {
            match &__sg_return {
                ::std::result::Result::Ok(value) => #rt::report_completion(__sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(value))),
                ::std::result::Result::Err(()) => #rt::report_completion(__sg_scope.error_unit(#ERROR_NAME)),
            }
        },
        ReturnKind::ResultUnits => quote! {
            match &__sg_return {
                ::std::result::Result::Ok(()) => #rt::report_completion(__sg_scope.unit()),
                ::std::result::Result::Err(()) => #rt::report_completion(__sg_scope.error_unit(#ERROR_NAME)),
            }
        },
    }
}

/// Expand a `spec_operation` attribute into capture instrumentation and metadata.
///
/// `attribute` contains the operation name plus optional component override;
/// `item` must be a supported free function or method. The returned tokens keep
/// the declaration's behavior while adding lazy input/result projection and
/// hygienic link-time operation metadata.
///
/// # Errors
///
/// Returns `syn::Error` for malformed attributes, unsupported parameter
/// patterns, or unsupported declarations.
///
/// # Panics
///
/// Panics only if the parser produces a non-unit return kind without an
/// explicit return type, which violates this implementation's internal model.
///
/// # Examples
///
/// ```
/// let attribute = quote::quote!("add", spec = "example.math");
/// let item = quote::quote!(fn add(left: i32, right: i32) -> i32 { left + right });
/// let expanded = specgate_annotations_macros_impl::operation::expand_operation(attribute, item)?;
/// assert!(!expanded.is_empty());
/// # Ok::<(), syn::Error>(())
/// ```
pub fn expand_operation(attribute: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let first: OperationArg = syn::parse2(attribute)?;
    let mut function: ItemFn = syn::parse2(item)?;
    let mut declarations = vec![first];
    let mut remaining_attributes = Vec::new();
    for attribute in std::mem::take(&mut function.attrs) {
        if attribute
            .path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "spec_operation")
        {
            declarations.push(attribute.parse_args::<OperationArg>()?);
        } else {
            remaining_attributes.push(attribute);
        }
    }
    function.attrs = remaining_attributes;
    let owner = declarations[0].component.clone();
    let generic_parameters = function
        .sig
        .generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => Some(Ok(parameter.ident.clone())),
            syn::GenericParam::Lifetime(_) => None,
            syn::GenericParam::Const(_) => Some(Err(syn::Error::new_spanned(
                parameter,
                "operation specializations do not support const parameters",
            ))),
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let mut names = std::collections::BTreeSet::new();
    let mut combinations = std::collections::BTreeSet::new();
    for declaration in &declarations {
        if declaration.component != owner || !names.insert(&declaration.name) {
            return Err(syn::Error::new_spanned(
                &function.sig,
                "operation specializations require one component and distinct names",
            ));
        }
        if declaration.types.len() != generic_parameters.len() {
            return Err(syn::Error::new_spanned(
                &function.sig,
                "declare one concrete `types = \"...\"` argument per generic type parameter",
            ));
        }
        let types = &declaration.types;
        if !combinations.insert(quote!(#(#types),*).to_string()) {
            return Err(syn::Error::new_spanned(&function.sig, "duplicate operation specialization types"));
        }
        let mut output = function.sig.output.clone();
        syn::visit_mut::VisitMut::visit_return_type_mut(
            &mut Substitution {
                parameters: &generic_parameters,
                arguments: &declaration.types,
            },
            &mut output,
        );
        if return_kind(&output) != return_kind(&function.sig.output) {
            return Err(syn::Error::new_spanned(
                output,
                "operation specialization changes the return outcome shape; declare a concrete return signature",
            ));
        }
    }
    let params = parameters(&mut function);
    let body = function.block.clone();
    let rt = runtime();
    let component = component(owner.as_deref());
    let name = if generic_parameters.is_empty() {
        let name = &declarations[0].name;
        quote!(::core::option::Option::Some(#name))
    } else {
        let cases = declarations.iter().map(|declaration| {
            let name = &declaration.name;
            let types = &declaration.types;
            quote! {
                if true #(&& ::core::any::TypeId::of::<#generic_parameters>() == ::core::any::TypeId::of::<#types>())* {
                    ::core::option::Option::Some(#name)
                } else
            }
        });
        quote!({ #(#cases)* { ::core::option::Option::None } })
    };
    let begin = begin_fragment(&rt, &component, &name, false);
    let begin_async = begin_fragment(&rt, &component, &name, true);
    let input_records = params
        .iter()
        .filter(|(_ident, ty, _name)| !is_mut_ref(ty))
        .map(|(ident, _ty, semantic_name)| {
            quote! {
                if let ::std::result::Result::Err(error) =
                    __sg_scope.input_lazy(#semantic_name, || #rt::ToNativeValue::to_native_value(&#ident))
                {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
            }
        })
        .collect::<Vec<_>>();
    let authored_return_type = match &function.sig.output {
        ReturnType::Default => "()".to_string(),
        ReturnType::Type(_, ty) => quote!(#ty).to_string(),
    };
    let is_async = function.sig.asyncness.is_some();
    let output_type: Type = match &function.sig.output {
        ReturnType::Default => parse_quote!(()),
        ReturnType::Type(_, ty) => (**ty).clone(),
    };
    let new_body = if is_async {
        let completion = async_completion(&rt, return_kind(&function.sig.output));
        function.sig.asyncness = None;
        function.sig.output = parse_quote!(-> impl ::core::future::Future<Output = #output_type>);
        parse_quote!({
            let __sg_context = #rt::capture_async_context();
            #rt::instrument_async_operation(__sg_context, async move {
                #begin_async
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = (async move #body).await;
                #completion
                __sg_return
            })
        })
    } else {
        match (&function.sig.output, return_kind(&function.sig.output)) {
            (_, ReturnKind::Unit) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> () #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                #rt::report_completion(__sg_scope.unit());
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::Option) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> #ty #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                match &__sg_return {
                    ::std::option::Option::Some(value) =>
                        #rt::report_completion(
                            __sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(value))
                        ),
                    ::std::option::Option::None =>
                        #rt::report_completion(__sg_scope.empty()),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::OptionUnit | ReturnKind::Value) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> #ty #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                #rt::report_completion(
                    __sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(&__sg_return))
                );
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::Result) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> #ty #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                match &__sg_return {
                    ::std::result::Result::Ok(value) =>
                        #rt::report_completion(
                            __sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(value))
                        ),
                    ::std::result::Result::Err(error_value) =>
                        #rt::report_completion(
                            __sg_scope.error_lazy(#ERROR_NAME, || #rt::ToNativeValue::to_native_value(error_value))
                        ),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::ResultUnit) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> #ty #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                match &__sg_return {
                    ::std::result::Result::Ok(()) =>
                        #rt::report_completion(__sg_scope.unit()),
                    ::std::result::Result::Err(error_value) =>
                        #rt::report_completion(
                            __sg_scope.error_lazy(#ERROR_NAME, || #rt::ToNativeValue::to_native_value(error_value))
                        ),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::ResultErrUnit) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> #ty #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                match &__sg_return {
                    ::std::result::Result::Ok(value) =>
                        #rt::report_completion(
                            __sg_scope.result_lazy(|| #rt::ToNativeValue::to_native_value(value))
                        ),
                    ::std::result::Result::Err(()) =>
                        #rt::report_completion(__sg_scope.error_unit(#ERROR_NAME)),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::ResultUnits) => parse_quote!({
                #begin
                #(#input_records)*
                if let ::std::result::Result::Err(error) = __sg_scope.inputs_recorded() {
                    #rt::report_error(#rt::Stage::OperationInput, &error);
                }
                let __sg_return = match ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(move || -> #ty #body)
                ) {
                    ::std::result::Result::Ok(value) => value,
                    ::std::result::Result::Err(payload) => {
                        #rt::report_completion(__sg_scope.unwind());
                        ::std::panic::resume_unwind(payload)
                    }
                };
                match &__sg_return {
                    ::std::result::Result::Ok(()) =>
                        #rt::report_completion(__sg_scope.unit()),
                    ::std::result::Result::Err(()) =>
                        #rt::report_completion(__sg_scope.error_unit(#ERROR_NAME)),
                }
                __sg_return
            }),
            _ => unreachable!("non-unit functions have explicit return types"),
        }
    };
    *function.block = new_body;

    let function_name = function.sig.ident.to_string();
    let is_method = has_receiver(&function);
    let is_public = matches!(function.vis, syn::Visibility::Public(_));
    let method_bridge = if is_method && std::env::var_os("SPECGATE_TEST_DISPATCH").is_some() {
        let package = std::env::var("CARGO_PKG_NAME").map_err(|error| syn::Error::new_spanned(&function.sig, error))?;
        let function_ident = &function.sig.ident;
        let bridge = quote::format_ident!("__SPECGATE_replay_{}", function_name.trim_start_matches("r#"));
        let visibility = &function.vis;
        let selections = declarations.iter().map(|declaration| {
            let name = &declaration.name;
            let types = &declaration.types;
            quote! {
                #name => {
                    macro_rules! __specgate_selected {
                        () => { __specgate_dispatch!(#package, method, #component, #name, #function_ident, [#(#types),*]) };
                    }
                    include!(env!("SPECGATE_TEST_DISPATCH"))
                }
            }
        });
        quote! {
            #[doc(hidden)]
            #[allow(dead_code, non_snake_case, reason = "generated operation-local replay bridge")]
            #visibility fn #bridge<'__specgate>(
                operation: &str,
                receiver: &'__specgate mut dyn ::std::any::Any,
                inputs: ::std::vec::Vec<#rt::Value>,
                setups: ::std::vec::Vec<::std::boxed::Box<dyn ::std::any::Any>>,
            ) -> ::std::pin::Pin<::std::boxed::Box<dyn ::core::future::Future<Output = ::core::result::Result<(), #rt::CaptureError>> + '__specgate>> {
                use #rt as __specgate;
                let invocation = match operation {
                    #(#selections,)*
                    _ => None::<__specgate::harness::Invocation>,
                };
                match invocation {
                    Some(invoke) => invoke(receiver, inputs, setups),
                    None => ::std::boxed::Box::pin(async {
                        Err(__specgate::harness::error("method has no selected lexical operation bridge"))
                    }),
                }
            }
        }
    } else {
        TokenStream2::new()
    };
    let metadata = declarations
        .iter()
        .map(|declaration| {
            let name = &declaration.name;
            let const_name = metadata_ident(METADATA_NAMESPACE, format!("{function_name}:{name}"), function.sig.ident.span());
            let mut substitution = Substitution {
                parameters: &generic_parameters,
                arguments: &declaration.types,
            };
            let parameter_metadata = params
                .iter()
                .map(|(_ident, ty, name)| {
                    let mut ty = ty.clone();
                    syn::visit_mut::VisitMut::visit_type_mut(&mut substitution, &mut ty);
                    let ty = quote!(#ty).to_string();
                    quote!(#rt::FieldMeta::new(
                        #rt::FieldName::new(#name),
                        #rt::RustType::new(#ty),
                    ))
                })
                .collect::<Vec<_>>();
            let return_type = if authored_return_type.is_empty() {
                String::new()
            } else {
                let mut output: Type = syn::parse_str(&authored_return_type)?;
                syn::visit_mut::VisitMut::visit_type_mut(&mut substitution, &mut output);
                quote!(#output).to_string()
            };
            let type_arguments = &declaration.types;
            let function_ident = &function.sig.ident;
            let replay = if !is_method && std::env::var_os("SPECGATE_TEST_DISPATCH").is_some() {
                let package = std::env::var("CARGO_PKG_NAME").map_err(|error| syn::Error::new_spanned(&function.sig, error))?;
                quote! {
                    .replay_operation({
                        use #rt as __specgate;
                        macro_rules! __specgate_selected {
                            () => { __specgate_dispatch!(#package, operation, #component, #name, #function_ident, [#(#type_arguments),*]) };
                        }
                        include!(env!("SPECGATE_TEST_DISPATCH"))
                    })
                }
            } else {
                TokenStream2::new()
            };
            let type_names = type_arguments.iter().map(|ty| quote!(#ty).to_string()).collect::<Vec<_>>();
            Ok(quote! {
            #[allow(dead_code, non_upper_case_globals, reason = "generated linker metadata is referenced externally")]
            const #const_name: () = {
                #[#rt::_private::linkme::distributed_slice(#rt::SPECGATE_OPS)]
                #[linkme(crate = #rt::_private::linkme)]
                static __SPECGATE_META: #rt::OpMeta = #rt::OpMeta::const_builder(#rt::OpDeps {
                    name: #rt::OpName::new(#name),
                    module_path: #rt::ModulePath::new(::core::module_path!()),
                    fn_name: #rt::FnName::new(#function_name),
                    params: &[#(#parameter_metadata),*],
                    return_type: #rt::RustType::new(#return_type),
                    component: #rt::ComponentName::new(#component),
                })
                .asynchronous(#is_async)
                .method(#is_method)
                .public(#is_public)
                .type_arguments(|| ::std::vec![#(#type_names),*])
                #replay
                .build();
            };
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(quote!(#function #method_bridge #(#metadata)*))
}

struct Substitution<'a> {
    parameters: &'a [syn::Ident],
    arguments: &'a [Type],
}

impl syn::visit_mut::VisitMut for Substitution<'_> {
    fn visit_type_mut(&mut self, i: &mut Type) {
        if let Type::Path(path) = i
            && path.qself.is_none()
            && path.path.segments.len() == 1
            && let Some(index) = self.parameters.iter().position(|parameter| path.path.is_ident(parameter))
        {
            *i = self.arguments[index].clone();
            return;
        }

        syn::visit_mut::visit_type_mut(self, i);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn specializations_cannot_change_completion_channels() {
        for function in [
            quote::quote!(
                fn make<T: 'static>() -> Option<T> {
                    None
                }
            ),
            quote::quote!(
                async fn make<T: 'static>() -> Option<T> {
                    None
                }
            ),
            quote::quote!(
                fn make<T: 'static>() -> Result<T, String> {
                    panic!()
                }
            ),
            quote::quote!(
                fn make<T: 'static>() -> T {
                    panic!()
                }
            ),
        ] {
            let error = super::expand_operation(quote::quote!("make", types = "()"), function).unwrap_err();
            assert!(error.to_string().contains("return outcome shape"), "{error}");
        }
        let error = super::expand_operation(
            quote::quote!("make", types = "Result<i32, String>"),
            quote::quote!(
                fn make<T: 'static>() -> T {
                    panic!()
                }
            ),
        )
        .unwrap_err();
        assert!(error.to_string().contains("return outcome shape"), "{error}");
    }

    #[test]
    fn duplicate_specialization_options_are_rejected() {
        for (attribute, diagnostic) in [
            (quote::quote!("find", types = "i32", types = "String"), "duplicate `types`"),
            (quote::quote!("find", spec = "one", spec = "two"), "duplicate `spec`"),
        ] {
            let error = super::expand_operation(
                attribute,
                quote::quote!(
                    fn find<T: 'static>() {}
                ),
            )
            .unwrap_err();
            assert!(error.to_string().contains(diagnostic), "{error}");
        }
    }
}
