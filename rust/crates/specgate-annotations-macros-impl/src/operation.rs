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
    let OperationArg { name, component: owner } = syn::parse2(attribute)?;
    let mut function: ItemFn = syn::parse2(item)?;
    let params = parameters(&mut function);
    let body = function.block.clone();
    let rt = runtime();
    let component = component(owner.as_deref());
    let begin = quote! {
        let mut __sg_scope = match #rt::begin_operation(
            #rt::ComponentId::from(#component),
            #rt::OperationName::from(#name),
        ) {
            ::std::result::Result::Ok(scope) => scope,
            ::std::result::Result::Err(error) => {
                #rt::report_error(#rt::Stage::OperationBegin, &error);
                #rt::OperationScope::inactive()
            }
        };
    };
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
                #begin
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
    let const_name = metadata_ident(METADATA_NAMESPACE, format!("{function_name}:{name}"), function.sig.ident.span());
    let is_method = has_receiver(&function);
    let is_public = matches!(function.vis, syn::Visibility::Public(_));
    let parameter_metadata = params.iter().map(|(_ident, ty, name)| {
        let ty = quote!(#ty).to_string();
        quote!(#rt::FieldMeta::new(
            #rt::FieldName::new(#name),
            #rt::RustType::new(#ty),
        ))
    });
    let return_type = authored_return_type;

    let expanded = quote! {
        #function

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
            .build();
        };
    };
    Ok(expanded)
}
