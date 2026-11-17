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
use syn::parse::{Parse, ParseStream, Parser as _};
use syn::{Data, DeriveInput, Fields, FnArg, Ident, ItemFn, LitStr, Pat, ReturnType, Token, Type, parse_quote};

pub mod harness;

// `Result::Err` has one unnamed Rust error channel. The annotations contract
// maps it to this stable CTSC declared-error name.
const ERROR_NAME: &str = "error";
// Generic argument positions in Result<T, E>.
const OK_ARG: usize = 0;
const ERROR_ARG: usize = 1;

struct OperationArg {
    name: String,
    component: Option<String>,
    types: Vec<Type>,
}

impl Parse for OperationArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse::<LitStr>()?.value();
        let mut component = None;
        let mut types = Vec::new();
        let mut has_types = false;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            let value = input.parse::<LitStr>()?.value();
            if key == "component" {
                if component.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate `component` argument"));
                }
                component = Some(value);
            } else if key == "spec" {
                return Err(syn::Error::new(
                    key.span(),
                    "`spec` is no longer supported; use `component` instead",
                ));
            } else if key == "types" {
                if has_types {
                    return Err(syn::Error::new(key.span(), "duplicate `types` argument"));
                }
                has_types = true;
                types = syn::punctuated::Punctuated::<Type, Token![,]>::parse_terminated
                    .parse_str(&value)?
                    .into_iter()
                    .collect();
            } else {
                return Err(syn::Error::new(key.span(), "expected `component` or `types`"));
            }
        }
        Ok(Self { name, component, types })
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
            } else if key == "component" {
                if component.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate `component` argument"));
                }
                component = Some(value);
            } else if key == "spec" {
                return Err(syn::Error::new(
                    key.span(),
                    "`spec` is no longer supported; use `component` instead",
                ));
            } else {
                return Err(syn::Error::new(key.span(), "expected `fills` or `component`"));
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

fn parameters(function: &mut ItemFn) -> syn::Result<Vec<(Ident, Type, String, bool)>> {
    collect_parameters(function, true)
}

/// Read every typed parameter's identifier, type, and language-neutral name.
///
/// `strip` removes the `#[spec_input]` markers once nothing else needs them.
/// Stacked `#[spec_setup]` annotations expand one attribute at a time, so every
/// expansion but the last must leave the markers in place for the next one.
fn collect_parameters(function: &mut ItemFn, strip: bool) -> syn::Result<Vec<(Ident, Type, String, bool)>> {
    let mut result = Vec::with_capacity(function.sig.inputs.len());
    for input in &mut function.sig.inputs {
        let FnArg::Typed(parameter) = input else {
            continue;
        };
        let Pat::Ident(pattern) = &*parameter.pat else {
            continue;
        };
        let mut semantic_name = pattern.ident.to_string();
        let mut ignored = false;
        for attribute in &parameter.attrs {
            if !attribute.path().is_ident("spec_input") {
                continue;
            }
            attribute.parse_args_with(|input: ParseStream| {
                if input.peek(LitStr) {
                    semantic_name = input.parse::<LitStr>()?.value();
                    if input.is_empty() {
                        return Ok(());
                    }
                    input.parse::<Token![,]>()?;
                }
                let marker = input.parse::<Ident>()?;
                if marker != "ignore" || ignored {
                    return Err(syn::Error::new(marker.span(), "expected a single `ignore` marker"));
                }
                ignored = true;
                Ok(())
            })?;
        }
        if strip {
            parameter.attrs.retain(|attribute| !attribute.path().is_ident("spec_input"));
        }
        result.push((pattern.ident.clone(), (*parameter.ty).clone(), semantic_name, ignored));
    }
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use quote::quote;

    #[test]
    fn component_selectors_preserve_other_options_and_crate_defaults() {
        let operation: super::OperationArg = syn::parse2(quote!("find", component = "example.math", types = "i32")).unwrap();
        assert_eq!(operation.component.as_deref(), Some("example.math"));
        assert_eq!(operation.types.len(), 1);
        let setup: super::SetupArg = syn::parse2(quote!("find", fills = "value", component = "example.math")).unwrap();
        assert_eq!(setup.component.as_deref(), Some("example.math"));
        assert_eq!(setup.fills.as_deref(), Some("value"));

        let operation: super::OperationArg = syn::parse2(quote!("find")).unwrap();
        let setup: super::SetupArg = syn::parse2(quote!("find")).unwrap();
        assert!(operation.component.is_none());
        assert!(setup.component.is_none());
    }

    #[test]
    fn duplicate_component_selectors_and_legacy_spellings_are_rejected() {
        for (attribute, diagnostic) in [
            (
                quote!("find", component = "one", component = "one"),
                "duplicate `component` argument",
            ),
            (
                quote!("find", component = "one", component = "two"),
                "duplicate `component` argument",
            ),
            (
                quote!("find", spec = "one"),
                "`spec` is no longer supported; use `component` instead",
            ),
            (
                quote!("find", spec = "one", component = "two"),
                "`spec` is no longer supported; use `component` instead",
            ),
            (
                quote!("find", component = "one", spec = "two"),
                "`spec` is no longer supported; use `component` instead",
            ),
        ] {
            let operation = syn::parse2::<super::OperationArg>(attribute.clone())
                .err()
                .expect("invalid operation selector");
            let setup = syn::parse2::<super::SetupArg>(attribute).err().expect("invalid setup selector");
            assert_eq!(operation.to_string(), diagnostic);
            assert_eq!(setup.to_string(), diagnostic);
        }
    }
}
