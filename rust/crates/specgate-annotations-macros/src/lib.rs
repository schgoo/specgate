//! Native CTSC annotation macros for `SpecGate`.
//!
//! Operations create real capture boundaries, setups and types register raw
//! link-time metadata, `SpecEvent` projects structured values through
//! `ToNativeValue`, and `spec_trace!` records native observations. Async
//! operations retain metadata but reject native capture before polling.

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Data, DeriveInput, Fields, FnArg, Ident, ItemFn, LitStr, Pat, ReturnType, Token, Type, parse_macro_input, parse_quote};

struct OperationArg {
    name: String,
    component: Option<String>,
}

impl Parse for OperationArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse::<LitStr>()?.value();
        let mut component = None;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            let value = input.parse::<LitStr>()?.value();
            if key == "spec" {
                component = Some(value);
            } else {
                return Err(syn::Error::new(key.span(), "expected `spec`"));
            }
        }
        Ok(Self { name, component })
    }
}

struct SetupArg {
    operation: String,
    fills: Option<String>,
    component: Option<String>,
}

impl Parse for SetupArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let operation = input.parse::<LitStr>()?.value();
        let mut fills = None;
        let mut component = None;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            let value = input.parse::<LitStr>()?.value();
            if key == "fills" {
                fills = Some(value);
            } else if key == "spec" {
                component = Some(value);
            } else {
                return Err(syn::Error::new(key.span(), "expected `fills` or `spec`"));
            }
        }
        Ok(Self {
            operation,
            fills,
            component,
        })
    }
}

struct TraceArgs {
    name: LitStr,
    value: syn::Expr,
}

impl Parse for TraceArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![,]>()?;
        let value = input.parse()?;
        Ok(Self { name, value })
    }
}

fn runtime() -> TokenStream2 {
    if let Ok(found) = crate_name("specgate") {
        return match found {
            FoundCrate::Itself => quote!(::specgate::__rt),
            FoundCrate::Name(name) => {
                let ident = Ident::new(&name, Span::call_site());
                quote!(::#ident::__rt)
            }
        };
    }
    quote!(::specgate::__rt)
}

fn component(component: Option<&str>) -> TokenStream2 {
    component.map_or_else(|| quote!(crate::__SPECGATE_COMPONENT), |value| quote!(#value))
}

fn has_receiver(function: &ItemFn) -> bool {
    function.sig.inputs.iter().any(|input| matches!(input, FnArg::Receiver(_)))
}

fn is_mutable_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(reference) if reference.mutability.is_some())
}

fn parameters(function: &mut ItemFn) -> Vec<(Ident, Type, String)> {
    let mut result = Vec::new();
    for input in &mut function.sig.inputs {
        let FnArg::Typed(parameter) = input else {
            continue;
        };
        let Pat::Ident(pattern) = &*parameter.pat else {
            continue;
        };
        let mut semantic_name = pattern.ident.to_string();
        parameter.attrs.retain(|attribute| {
            if !attribute.path().is_ident("spec_input") {
                return true;
            }
            if let Ok(name) = attribute.parse_args::<LitStr>() {
                semantic_name = name.value();
            }
            false
        });
        result.push((pattern.ident.clone(), (*parameter.ty).clone(), semantic_name));
    }
    result
}

enum ReturnKind {
    Unit,
    Option,
    OptionUnit,
    Result,
    ResultUnit,
    ResultErrorUnit,
    ResultBothUnit,
    Value,
}

fn return_kind(output: &ReturnType) -> ReturnKind {
    match output {
        ReturnType::Default => ReturnKind::Unit,
        ReturnType::Type(_, ty) => match &**ty {
            Type::Tuple(tuple) if tuple.elems.is_empty() => ReturnKind::Unit,
            Type::Path(path) => {
                let segment = path.path.segments.last();
                match segment.map(|segment| segment.ident.to_string()).as_deref() {
                    Some("Option") if segment.is_some_and(|segment| type_argument_is_unit(segment, 0)) => ReturnKind::OptionUnit,
                    Some("Option") => ReturnKind::Option,
                    Some("Result")
                        if segment.is_some_and(|segment| type_argument_is_unit(segment, 0) && type_argument_is_unit(segment, 1)) =>
                    {
                        ReturnKind::ResultBothUnit
                    }
                    Some("Result") if segment.is_some_and(|segment| type_argument_is_unit(segment, 0)) => ReturnKind::ResultUnit,
                    Some("Result") if segment.is_some_and(|segment| type_argument_is_unit(segment, 1)) => ReturnKind::ResultErrorUnit,
                    Some("Result") => ReturnKind::Result,
                    _ => ReturnKind::Value,
                }
            }
            _ => ReturnKind::Value,
        },
    }
}

fn type_argument_is_unit(segment: &syn::PathSegment, index: usize) -> bool {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    matches!(
        arguments.args.get(index),
        Some(syn::GenericArgument::Type(Type::Tuple(tuple))) if tuple.elems.is_empty()
    )
}

/// Mark a function or method as a native CTSC operation boundary.
#[proc_macro_attribute]
pub fn spec_operation(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let OperationArg { name, component: owner } = parse_macro_input!(attribute as OperationArg);
    let mut function = parse_macro_input!(item as ItemFn);
    let params = parameters(&mut function);
    let body = function.block.clone();
    let rt = runtime();
    let component = component(owner.as_deref());
    let begin = quote! {
        let mut __sg_scope = #rt::begin_native_operation(#component, #name)
            .unwrap_or_else(|error| panic!("failed to begin native operation: {error}"));
    };
    let input_records = params
        .iter()
        .filter(|(_ident, ty, _name)| !is_mutable_reference(ty))
        .map(|(ident, _ty, semantic_name)| {
            quote! {
                __sg_scope
                    .record_input(#semantic_name, #rt::ToNativeValue::to_native_value(&#ident))
                    .unwrap_or_else(|error| panic!("failed to record native operation input: {error}"));
            }
        })
        .collect::<Vec<_>>();
    let is_async = function.sig.asyncness.is_some();
    let new_body = if is_async {
        parse_quote!({
            #rt::reject_async_native_capture(#component, #name)
                .unwrap_or_else(|error| panic!("{error}"));
            (async move #body).await
        })
    } else {
        match (&function.sig.output, return_kind(&function.sig.output)) {
            (_, ReturnKind::Unit) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> () #body)();
                __sg_scope
                    .complete_unit()
                    .unwrap_or_else(|error| panic!("failed to complete native unit operation: {error}"));
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::Option) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                match &__sg_return {
                    ::std::option::Option::Some(value) => __sg_scope
                        .complete_result(#rt::ToNativeValue::to_native_value(value))
                        .unwrap_or_else(|error| panic!("failed to complete native optional operation: {error}")),
                    ::std::option::Option::None => __sg_scope
                        .complete_empty()
                        .unwrap_or_else(|error| panic!("failed to complete native empty operation: {error}")),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::OptionUnit) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                __sg_scope
                    .complete_result(#rt::ToNativeValue::to_native_value(&__sg_return))
                    .unwrap_or_else(|error| panic!("failed to complete native optional unit operation: {error}"));
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::Result) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                match &__sg_return {
                    ::std::result::Result::Ok(value) => __sg_scope
                        .complete_result(#rt::ToNativeValue::to_native_value(value))
                        .unwrap_or_else(|error| panic!("failed to complete native result operation: {error}")),
                    ::std::result::Result::Err(error_value) => __sg_scope
                        .complete_error("error", #rt::ToNativeValue::to_native_value(error_value))
                        .unwrap_or_else(|error| panic!("failed to complete native declared error: {error}")),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::ResultUnit) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                match &__sg_return {
                    ::std::result::Result::Ok(()) => __sg_scope
                        .complete_unit()
                        .unwrap_or_else(|error| panic!("failed to complete native unit result operation: {error}")),
                    ::std::result::Result::Err(error_value) => __sg_scope
                        .complete_error("error", #rt::ToNativeValue::to_native_value(error_value))
                        .unwrap_or_else(|error| panic!("failed to complete native declared error: {error}")),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::ResultErrorUnit) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                match &__sg_return {
                    ::std::result::Result::Ok(value) => __sg_scope
                        .complete_result(#rt::ToNativeValue::to_native_value(value))
                        .unwrap_or_else(|error| panic!("failed to complete native result operation: {error}")),
                    ::std::result::Result::Err(()) => __sg_scope
                        .complete_error_unit("error")
                        .unwrap_or_else(|error| panic!("failed to complete native valueless declared error: {error}")),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::ResultBothUnit) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                match &__sg_return {
                    ::std::result::Result::Ok(()) => __sg_scope
                        .complete_unit()
                        .unwrap_or_else(|error| panic!("failed to complete native unit result operation: {error}")),
                    ::std::result::Result::Err(()) => __sg_scope
                        .complete_error_unit("error")
                        .unwrap_or_else(|error| panic!("failed to complete native valueless declared error: {error}")),
                }
                __sg_return
            }),
            (ReturnType::Type(_, ty), ReturnKind::Value) => parse_quote!({
                #begin
                #(#input_records)*
                let __sg_return = (move || -> #ty #body)();
                __sg_scope
                    .complete_result(#rt::ToNativeValue::to_native_value(&__sg_return))
                    .unwrap_or_else(|error| panic!("failed to complete native result operation: {error}"));
                __sg_return
            }),
            _ => unreachable!("non-unit functions have explicit return types"),
        }
    };
    *function.block = new_body;

    let function_name = function.sig.ident.to_string();
    let suffix = sanitize_identifier(&format!("{function_name}_{name}"));
    let const_name = Ident::new(&format!("_SPECGATE_OPERATION_{suffix}"), function.sig.ident.span());
    let static_name = Ident::new(&format!("_SPECGATE_OPERATION_META_{suffix}"), function.sig.ident.span());
    let is_method = has_receiver(&function);
    let is_public = matches!(function.vis, syn::Visibility::Public(_));
    let parameter_metadata = params.iter().map(|(_ident, ty, name)| {
        let ty = quote!(#ty).to_string();
        quote!((#name, #ty))
    });
    let return_type = match &function.sig.output {
        ReturnType::Default => "()".to_string(),
        ReturnType::Type(_, ty) => quote!(#ty).to_string(),
    };

    quote! {
        #function

        #[allow(dead_code, non_upper_case_globals)]
        const #const_name: () = {
            #[#rt::linkme::distributed_slice(#rt::SPECGATE_OPS)]
            #[linkme(crate = #rt::linkme)]
            static #static_name: #rt::OpMeta = #rt::OpMeta {
                name: #name,
                module_path: ::core::module_path!(),
                fn_name: #function_name,
                is_setup: false,
                is_async: #is_async,
                is_method: #is_method,
                is_public: #is_public,
                params: &[#(#parameter_metadata),*],
                return_type: #return_type,
                fills: "",
                component: #component,
            };
        };
    }
    .into()
}

/// Register a deterministic setup producer without modifying its behavior.
#[proc_macro_attribute]
pub fn spec_setup(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let SetupArg {
        operation,
        fills,
        component: owner,
    } = parse_macro_input!(attribute as SetupArg);
    let mut function = parse_macro_input!(item as ItemFn);
    let params = parameters(&mut function);
    let rt = runtime();
    let component = component(owner.as_deref());
    let function_name = function.sig.ident.to_string();
    let fills = fills.unwrap_or_default();
    let suffix = sanitize_identifier(&format!("{function_name}_{operation}_{fills}"));
    let const_name = Ident::new(&format!("_SPECGATE_SETUP_{suffix}"), function.sig.ident.span());
    let static_name = Ident::new(&format!("_SPECGATE_SETUP_META_{suffix}"), function.sig.ident.span());
    let is_async = function.sig.asyncness.is_some();
    let is_public = matches!(function.vis, syn::Visibility::Public(_));
    let parameter_metadata = params.iter().map(|(_ident, ty, name)| {
        let ty = quote!(#ty).to_string();
        quote!((#name, #ty))
    });
    let return_type = match &function.sig.output {
        ReturnType::Default => "()".to_string(),
        ReturnType::Type(_, ty) => quote!(#ty).to_string(),
    };
    quote! {
        #function

        #[allow(dead_code, non_upper_case_globals)]
        const #const_name: () = {
            #[#rt::linkme::distributed_slice(#rt::SPECGATE_OPS)]
            #[linkme(crate = #rt::linkme)]
            static #static_name: #rt::OpMeta = #rt::OpMeta {
                name: #operation,
                module_path: ::core::module_path!(),
                fn_name: #function_name,
                is_setup: true,
                is_async: #is_async,
                is_method: false,
                is_public: #is_public,
                params: &[#(#parameter_metadata),*],
                return_type: #return_type,
                fills: #fills,
                component: #component,
            };
        };
    }
    .into()
}

/// Derive native semantic projection and link-time type metadata.
#[proc_macro_derive(SpecEvent, attributes(spec_event, spec_component))]
pub fn derive_spec_event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let rt = runtime();
    let owner = input
        .attrs
        .iter()
        .find(|attribute| attribute.path().is_ident("spec_component"))
        .and_then(|attribute| attribute.parse_args::<LitStr>().ok())
        .map(|literal| literal.value());
    let component = component(owner.as_deref());
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let type_name = name.to_string();
    let suffix = sanitize_identifier(&type_name);

    let (projection, fields, variants, kind) = match &input.data {
        Data::Struct(data) => {
            let Fields::Named(named) = &data.fields else {
                return syn::Error::new_spanned(name, "SpecEvent structs must use named fields")
                    .to_compile_error()
                    .into();
            };
            let selected = named
                .named
                .iter()
                .filter_map(|field| {
                    let ident = field.ident.as_ref()?;
                    event_field_name(&field.attrs, &ident.to_string()).map(|semantic_name| (ident, &field.ty, semantic_name))
                })
                .collect::<Vec<_>>();
            let values_ident = hygienic_ident("__specgate_struct_values");
            let inserts = selected.iter().map(|(ident, _ty, semantic_name)| {
                quote! {
                    #values_ident.insert(
                        #semantic_name.to_string(),
                        #rt::ToNativeValue::to_native_value(&self.#ident),
                    );
                }
            });
            let metadata = selected.iter().map(|(_ident, ty, semantic_name)| {
                let ty = quote!(#ty).to_string();
                quote!((#semantic_name, #ty))
            });
            (
                quote! {
                    let mut #values_ident = ::std::collections::BTreeMap::new();
                    #(#inserts)*
                    #rt::Value::Map(#values_ident)
                },
                quote!(&[#(#metadata),*]),
                quote!(&[]),
                "struct",
            )
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            let mut variant_metadata = Vec::new();
            for (variant_index, variant) in data.variants.iter().enumerate() {
                let variant_ident = &variant.ident;
                let variant_name = variant_ident.to_string();
                match &variant.fields {
                    Fields::Unit => {
                        arms.push(quote! {
                            Self::#variant_ident => #rt::Value::Map(::std::collections::BTreeMap::from([(
                                #variant_name.to_string(),
                                #rt::Value::Map(::std::collections::BTreeMap::new()),
                            )]))
                        });
                        variant_metadata.push(quote!(#rt::VariantMeta {
                            name: #variant_name,
                            fields: &[],
                            tuple: ::std::option::Option::None,
                        }));
                    }
                    Fields::Named(named) => {
                        let selected = named
                            .named
                            .iter()
                            .enumerate()
                            .filter_map(|(field_index, field)| {
                                let ident = field.ident.as_ref()?;
                                Some((
                                    ident,
                                    hygienic_ident(&format!("__specgate_variant_{variant_index}_field_{field_index}")),
                                    &field.ty,
                                    event_field_name(&field.attrs, &ident.to_string()).unwrap_or_else(|| ident.to_string()),
                                ))
                            })
                            .collect::<Vec<_>>();
                        let payload_ident = hygienic_ident(&format!("__specgate_variant_{variant_index}_payload"));
                        let patterns = selected.iter().map(|(ident, binding, _ty, _name)| quote!(#ident: #binding));
                        let inserts = selected.iter().map(|(_ident, binding, _ty, semantic_name)| {
                            quote! {
                                #payload_ident.insert(
                                    #semantic_name.to_string(),
                                    #rt::ToNativeValue::to_native_value(#binding),
                                );
                            }
                        });
                        let metadata = selected.iter().map(|(_ident, _binding, ty, semantic_name)| {
                            let ty = quote!(#ty).to_string();
                            quote!((#semantic_name, #ty))
                        });
                        arms.push(quote! {
                            Self::#variant_ident { #(#patterns),*, .. } => {
                                let mut #payload_ident = ::std::collections::BTreeMap::new();
                                #(#inserts)*
                                #rt::Value::Map(::std::collections::BTreeMap::from([(
                                    #variant_name.to_string(),
                                    #rt::Value::Map(#payload_ident),
                                )]))
                            }
                        });
                        variant_metadata.push(quote!(#rt::VariantMeta {
                            name: #variant_name,
                            fields: &[#(#metadata),*],
                            tuple: ::std::option::Option::None,
                        }));
                    }
                    Fields::Unnamed(unnamed) => {
                        let bindings = (0..unnamed.unnamed.len())
                            .map(|field_index| hygienic_ident(&format!("__specgate_variant_{variant_index}_field_{field_index}")))
                            .collect::<Vec<_>>();
                        let values = bindings.iter().map(|binding| quote!(#rt::ToNativeValue::to_native_value(#binding)));
                        let tuple = unnamed.unnamed.iter().map(|field| {
                            let ty = &field.ty;
                            let ty = quote!(#ty).to_string();
                            quote!(#ty)
                        });
                        arms.push(quote! {
                            Self::#variant_ident(#(#bindings),*) => #rt::Value::Map(::std::collections::BTreeMap::from([(
                                #variant_name.to_string(),
                                #rt::Value::List(vec![#(#values),*]),
                            )]))
                        });
                        variant_metadata.push(quote!(#rt::VariantMeta {
                            name: #variant_name,
                            fields: &[],
                            tuple: ::std::option::Option::Some(&[#(#tuple),*]),
                        }));
                    }
                }
            }
            (
                quote!(match self { #(#arms),* }),
                quote!(&[]),
                quote!(&[#(#variant_metadata),*]),
                "enum",
            )
        }
        Data::Union(_) => {
            return syn::Error::new_spanned(name, "SpecEvent supports structs and enums only")
                .to_compile_error()
                .into();
        }
    };
    let const_name = Ident::new(&format!("_SPECGATE_TYPE_{suffix}"), name.span());
    let static_name = Ident::new(&format!("_SPECGATE_TYPE_META_{suffix}"), name.span());
    quote! {
        impl #impl_generics #rt::ToNativeValue for #name #type_generics #where_clause {
            fn to_native_value(&self) -> #rt::Value {
                #projection
            }
        }

        impl #impl_generics #rt::SpecEvent for #name #type_generics #where_clause {}

        #[allow(dead_code, non_upper_case_globals)]
        const #const_name: () = {
            #[#rt::linkme::distributed_slice(#rt::SPECGATE_TYPES)]
            #[linkme(crate = #rt::linkme)]
            static #static_name: #rt::TypeMeta = #rt::TypeMeta {
                name: #type_name,
                module_path: ::core::module_path!(),
                kind: #kind,
                fields: #fields,
                variants: #variants,
                component: #component,
            };
        };
    }
    .into()
}

fn event_field_name(attributes: &[syn::Attribute], default: &str) -> Option<String> {
    let attribute = attributes.iter().find(|attribute| attribute.path().is_ident("spec_event"))?;
    if attribute.meta.require_list().is_err() {
        return Some(default.to_string());
    }
    if let Ok(literal) = attribute.parse_args::<LitStr>() {
        return Some(literal.value());
    }
    let mut name = None;
    let _ = attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("name") {
            name = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else {
            Err(meta.error("expected `name`"))
        }
    });
    Some(name.unwrap_or_else(|| default.to_string()))
}

/// Record one native observation in the active operation.
#[proc_macro]
pub fn spec_trace(input: TokenStream) -> TokenStream {
    let TraceArgs { name, value } = parse_macro_input!(input as TraceArgs);
    let rt = runtime();
    quote! {
        #rt::emit_event(#name, &(#value));
    }
    .into()
}

/// Declare the crate's default component identifier.
#[proc_macro]
pub fn spec_component(input: TokenStream) -> TokenStream {
    let component = parse_macro_input!(input as LitStr);
    quote! {
        #[doc(hidden)]
        pub const __SPECGATE_COMPONENT: &str = #component;
    }
    .into()
}

fn sanitize_identifier(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn hygienic_ident(value: &str) -> Ident {
    Ident::new(value, Span::mixed_site())
}
