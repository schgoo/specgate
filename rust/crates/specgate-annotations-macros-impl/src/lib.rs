//! Expansion implementation for `SpecGate` annotation macros.
//!
//! Operations create real capture boundaries; setups register link-time
//! metadata and record the construction inputs the registry folds into the
//! operation they build; types register raw link-time metadata; `SpecEvent`
//! projects structured values through `ToNativeValue`; and `spec_trace!`
//! records native observations. Async operations capture their construction context,
//! open at first poll, and reinstall that context on every poll. Async setups remain
//! uninstrumented and are rejected by capture planning.

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Data, DeriveInput, Fields, FnArg, Ident, ItemFn, LitStr, Pat, ReturnType, Token, Type, parse_quote};

// `Result::Err` has one unnamed Rust error channel. The annotations contract
// maps it to this stable CTSC declared-error name.
const ERROR_NAME: &str = "error";
// Generic argument positions in Result<T, E>.
const OK_ARG: usize = 0;
const ERROR_ARG: usize = 1;

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

fn runtime() -> TokenStream2 {
    runtime_with(crate_name)
}

fn runtime_with(resolve: impl FnOnce(&str) -> Result<FoundCrate, proc_macro_crate::Error>) -> TokenStream2 {
    if let Ok(found) = resolve("specgate") {
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

fn metadata_ident(prefix: impl AsRef<str>, identity: impl AsRef<str>, span: Span) -> Ident {
    // These stable FNV-1a parameters define generated metadata symbol identities;
    // changing either requires coordinated compatibility review.
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let prefix = prefix.as_ref();
    let mut hash = FNV_OFFSET_BASIS;
    for byte in identity.as_ref().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    Ident::new(&format!("__SPECGATE_{prefix}_{hash:016X}"), span)
}

fn component(value: Option<impl AsRef<str>>) -> TokenStream2 {
    value.map_or_else(
        || quote!(crate::__SPECGATE_COMPONENT),
        |value| {
            let value = value.as_ref();
            quote!(#value)
        },
    )
}

fn has_receiver(function: &ItemFn) -> bool {
    function.sig.inputs.iter().any(|input| matches!(input, FnArg::Receiver(_)))
}

fn is_mut_ref(ty: &Type) -> bool {
    matches!(ty, Type::Reference(reference) if reference.mutability.is_some())
}

fn parameters(function: &mut ItemFn) -> Vec<(Ident, Type, String)> {
    collect_parameters(function, true)
}

/// Read every typed parameter's identifier, type, and language-neutral name.
///
/// `strip` removes the `#[spec_input]` markers once nothing else needs them.
/// Stacked `#[spec_setup]` annotations expand one attribute at a time, so every
/// expansion but the last must leave the markers in place for the next one.
fn collect_parameters(function: &mut ItemFn, strip: bool) -> Vec<(Ident, Type, String)> {
    let mut result = Vec::with_capacity(function.sig.inputs.len());
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
            !strip
        });
        result.push((pattern.ident.clone(), (*parameter.ty).clone(), semantic_name));
    }
    result
}

#[derive(Clone, Copy)]
enum ReturnKind {
    Unit,
    Option,
    OptionUnit,
    Result,
    ResultUnit,
    ResultErrUnit,
    ResultUnits,
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
                    Some("Option") if segment.is_some_and(|segment| is_unit_arg(segment, OK_ARG)) => ReturnKind::OptionUnit,
                    Some("Option") => ReturnKind::Option,
                    Some("Result") if segment.is_some_and(|segment| is_unit_arg(segment, OK_ARG) && is_unit_arg(segment, ERROR_ARG)) => {
                        ReturnKind::ResultUnits
                    }
                    Some("Result") if segment.is_some_and(|segment| is_unit_arg(segment, OK_ARG)) => ReturnKind::ResultUnit,
                    Some("Result") if segment.is_some_and(|segment| is_unit_arg(segment, ERROR_ARG)) => ReturnKind::ResultErrUnit,
                    Some("Result") => ReturnKind::Result,
                    _ => ReturnKind::Value,
                }
            }
            _ => ReturnKind::Value,
        },
    }
}

fn is_unit_arg(segment: &syn::PathSegment, index: usize) -> bool {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    matches!(
        arguments.args.get(index),
        Some(syn::GenericArgument::Type(Type::Tuple(tuple))) if tuple.elems.is_empty()
    )
}

/// `SpecEvent` derive expansion with projection and metadata examples.
pub mod event;
/// Operation attribute expansion with instrumentation examples.
pub mod operation;
/// Setup attribute expansion with registration examples.
pub mod setup;
