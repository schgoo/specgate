//! `SpecEvent` derive expansion.
//!
//! [`expand_event`] accepts one complete struct or enum declaration and emits
//! native-value projection plus link-time type metadata. Use it through the
//! proc-macro facade unless testing expansion behavior directly.

use std::fmt::Write as _;

use super::{Data, DeriveInput, Fields, Ident, LitStr, Span, TokenStream2, component, quote, runtime};

/// Expand a `SpecEvent` derive input into semantic projection and type metadata.
///
/// `input` must be a complete struct or enum declaration token stream. The
/// returned tokens implement the facade's native projection traits and emit
/// hygienic link-time type metadata without changing the declaration.
///
/// # Examples
///
/// ```
/// let input = quote::quote!(struct Point { x: i32 });
/// let expanded = specgate_annotations_macros_impl::event::expand_event(input)?;
/// let file: syn::File = syn::parse2(expanded)?;
/// assert_eq!(file.items.iter().filter(|item| matches!(item, syn::Item::Impl(_))).count(), 2);
/// assert!(file.items.iter().any(|item| matches!(item, syn::Item::Const(_))));
/// # Ok::<(), syn::Error>(())
/// ```
///
/// # Errors
///
/// Returns `syn::Error` when the input is malformed or uses unsupported field
/// attributes or data shapes.
pub fn expand_event(input: TokenStream2) -> syn::Result<TokenStream2> {
    let input: DeriveInput = syn::parse2(input)?;
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
    let mut type_name = name.to_string();
    let mut property_setup = false;
    for attribute in input.attrs.iter().filter(|attribute| attribute.path().is_ident("spec_event")) {
        if let Ok(literal) = attribute.parse_args::<LitStr>() {
            type_name = literal.value();
        } else if attribute.meta.require_list().is_ok() {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("setup") {
                    property_setup = true;
                } else if meta.path.is_ident("name") {
                    type_name = meta.value()?.parse::<LitStr>()?.value();
                } else {
                    return Err(meta.error("expected `name` or `setup`"));
                }
                Ok(())
            })?;
        }
    }
    if property_setup && (!input.generics.params.is_empty() || !matches!(input.data, Data::Struct(_))) {
        return Err(syn::Error::new_spanned(
            name,
            "property setups require a nongeneric named-field struct",
        ));
    }
    let mut reconstruction = TokenStream2::new();

    let (projection, fields, variants, kind) = match &input.data {
        Data::Struct(data) => {
            let Fields::Named(named) = &data.fields else {
                return Err(syn::Error::new_spanned(name, "SpecEvent structs must use named fields"));
            };
            let annotated = named
                .named
                .iter()
                .any(|field| field.attrs.iter().any(|attribute| attribute.path().is_ident("spec_event")));
            let mut selected = Vec::new();
            let mut ignored = Vec::new();
            for field in &named.named {
                let Some(ident) = field.ident.as_ref() else { continue };
                let options = event_field(&field.attrs, ident.to_string())?;
                if options.as_ref().is_some_and(|options| options.ignored) {
                    ignored.push(field);
                } else if let Some(options) = options.or_else(|| (property_setup && !annotated).then(|| EventField::new(ident.to_string())))
                {
                    selected.push((ident, &field.ty, options.name, options.path));
                }
            }
            if property_setup {
                let mut semantic_names = std::collections::BTreeSet::new();
                for (ident, _, semantic_name, path_projection) in &selected {
                    if let Some(field) = named.named.iter().find(|field| field.ident.as_ref() == Some(*ident))
                        && !matches!(field.vis, syn::Visibility::Public(_))
                    {
                        return Err(syn::Error::new_spanned(
                            field,
                            "property setup inputs must be public so generated replay runners can reconstruct them",
                        ));
                    }
                    if *path_projection {
                        return Err(syn::Error::new_spanned(
                            name,
                            "property setup fields cannot use a lossy path projection; use an explicit producer",
                        ));
                    }
                    if !semantic_names.insert(semantic_name) {
                        return Err(syn::Error::new_spanned(
                            name,
                            format!("property setup produces duplicate input name '{semantic_name}'"),
                        ));
                    }
                }
                let locals = selected
                    .iter()
                    .enumerate()
                    .map(|(index, _)| hygienic_ident(format!("__specgate_input_{index}")))
                    .collect::<Vec<_>>();
                let decode = selected.iter().zip(&locals).map(|((_ident, ty, semantic_name, _), local)| {
                    quote! {
                        let #local = <#ty as #rt::harness::Input>::decode(
                            values.remove(#semantic_name).ok_or_else(|| #rt::harness::error(
                                concat!("missing property setup input '", #semantic_name, "'")
                            ))?
                        )?;
                    }
                });
                let bindings = selected
                    .iter()
                    .zip(&locals)
                    .map(|((ident, _, _, _), local)| (*ident, local))
                    .collect::<Vec<_>>();
                let fields = bindings.iter().map(|(ident, local)| quote!(#ident: #local)).collect::<Vec<_>>();
                let construction = if selected.len() == named.named.len() {
                    quote!(Self { #(#fields,)* })
                } else if selected.len() + ignored.len() == named.named.len() {
                    ignored_defaults(&ignored, &bindings)
                } else {
                    quote!(Self { #(#fields,)* ..::core::default::Default::default() })
                };
                reconstruction = quote! {
                    impl #rt::harness::Input for #name {
                        fn decode(value: #rt::Value) -> ::core::result::Result<Self, #rt::CaptureError> {
                            let #rt::Value::Map(mut values) = value else {
                                return ::core::result::Result::Err(#rt::harness::error("expected property setup inputs"));
                            };
                            #(#decode)*
                            if !values.is_empty() {
                                return ::core::result::Result::Err(#rt::harness::error("unexpected property setup inputs"));
                            }
                            ::core::result::Result::Ok(#construction)
                        }
                    }
                };
            }
            let values_ident = hygienic_ident("__specgate_struct_values");
            let inserts = selected.iter().map(|(ident, _ty, semantic_name, path_projection)| {
                let value = if *path_projection {
                    quote!(#rt::Value::String(self.#ident.display().to_string()))
                } else {
                    quote!(#rt::ToNativeValue::to_native_value(&self.#ident))
                };
                quote! {
                    #values_ident.insert(#semantic_name.to_string(), #value);
                }
            });
            let metadata = selected.iter().map(|(_ident, ty, semantic_name, path_projection)| {
                let ty = if *path_projection {
                    "String".to_string()
                } else {
                    quote!(#ty).to_string()
                };
                quote!(#rt::FieldMeta::new(
                    #rt::FieldName::new(#semantic_name),
                    #rt::RustType::new(#ty),
                ))
            });
            (
                quote! {
                    let mut #values_ident = ::std::collections::BTreeMap::new();
                    #(#inserts)*
                    #rt::Value::Map(#values_ident)
                },
                quote!(&[#(#metadata),*]),
                quote!(&[]),
                quote!(#rt::TypeKind::Struct),
            )
        }
        Data::Enum(data) => {
            let mut arms = Vec::with_capacity(data.variants.len());
            let mut variant_metadata = Vec::with_capacity(data.variants.len());
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
                            name: #rt::VariantName::new(#variant_name),
                            fields: &[],
                            tuple: ::std::option::Option::None,
                        }));
                    }
                    Fields::Named(named) => {
                        let mut selected = Vec::new();
                        for (field_index, field) in named.named.iter().enumerate() {
                            let Some(ident) = field.ident.as_ref() else { continue };
                            let options =
                                event_field(&field.attrs, ident.to_string())?.unwrap_or_else(|| EventField::new(ident.to_string()));
                            if !options.ignored {
                                selected.push((
                                    ident,
                                    hygienic_ident(format!("__specgate_variant_{variant_index}_field_{field_index}")),
                                    &field.ty,
                                    options.name,
                                ));
                            }
                        }
                        let payload_ident = hygienic_ident(format!("__specgate_variant_{variant_index}_payload"));
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
                            quote!(#rt::FieldMeta::new(
                                #rt::FieldName::new(#semantic_name),
                                #rt::RustType::new(#ty),
                            ))
                        });
                        arms.push(quote! {
                            Self::#variant_ident { #(#patterns,)* .. } => {
                                let mut #payload_ident = ::std::collections::BTreeMap::new();
                                #(#inserts)*
                                #rt::Value::Map(::std::collections::BTreeMap::from([(
                                    #variant_name.to_string(),
                                    #rt::Value::Map(#payload_ident),
                                )]))
                            }
                        });
                        variant_metadata.push(quote!(#rt::VariantMeta {
                            name: #rt::VariantName::new(#variant_name),
                            fields: &[#(#metadata),*],
                            tuple: ::std::option::Option::None,
                        }));
                    }
                    Fields::Unnamed(unnamed) => {
                        // Each generated identifier carries decimal variant and field indices.
                        const INDEX_DIGITS: usize = usize::MAX.ilog10() as usize + 1;
                        const IDENT_CAP: usize = "__specgate_variant__field_".len() + INDEX_DIGITS + INDEX_DIGITS;
                        let mut identifier = String::with_capacity(IDENT_CAP);
                        let bindings = (0..unnamed.unnamed.len())
                            .map(|field_index| {
                                identifier.clear();
                                write!(identifier, "__specgate_variant_{variant_index}_field_{field_index}")
                                    .expect("writing to a String cannot fail");
                                hygienic_ident(&identifier)
                            })
                            .collect::<Vec<_>>();
                        let options = unnamed
                            .unnamed
                            .iter()
                            .map(|field| event_field(&field.attrs, ""))
                            .collect::<syn::Result<Vec<_>>>()?;
                        let selected = options
                            .iter()
                            .map(|options| !options.as_ref().is_some_and(|options| options.ignored))
                            .collect::<Vec<_>>();
                        let patterns = bindings
                            .iter()
                            .zip(&selected)
                            .map(|(binding, selected)| if *selected { quote!(#binding) } else { quote!(_) });
                        let values = bindings
                            .iter()
                            .zip(&selected)
                            .filter(|(_, selected)| **selected)
                            .map(|(binding, _)| quote!(#rt::ToNativeValue::to_native_value(#binding)));
                        let tuple = unnamed
                            .unnamed
                            .iter()
                            .zip(&selected)
                            .filter(|(_, selected)| **selected)
                            .map(|(field, _)| {
                                let ty = &field.ty;
                                let ty = quote!(#ty).to_string();
                                quote!(#rt::RustType::new(#ty))
                            });
                        arms.push(quote! {
                            Self::#variant_ident(#(#patterns),*) => #rt::Value::Map(::std::collections::BTreeMap::from([(
                                #variant_name.to_string(),
                                #rt::Value::List(vec![#(#values),*]),
                            )]))
                        });
                        variant_metadata.push(quote!(#rt::VariantMeta {
                            name: #rt::VariantName::new(#variant_name),
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
                quote!(#rt::TypeKind::Enum),
            )
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(name, "SpecEvent supports structs and enums only"));
        }
    };
    let property_metadata = property_setup.then(|| quote!(.property_setup(#rt::RustType::new(stringify!(#name)))));
    let expanded = quote! {
        #reconstruction
        impl #impl_generics #rt::ToNativeValue for #name #type_generics #where_clause {
            fn to_native_value(&self) -> #rt::Value {
                #projection
            }
        }

        impl #impl_generics #rt::SpecEvent for #name #type_generics #where_clause {}

        #[allow(dead_code, non_upper_case_globals, reason = "generated linker metadata is referenced externally")]
        const _: () = {
            #[#rt::_private::linkme::distributed_slice(#rt::SPECGATE_TYPES)]
            #[linkme(crate = #rt::_private::linkme)]
            static __SPECGATE_META: #rt::TypeMeta = #rt::TypeMeta::const_builder(#rt::TypeDeps {
                name: #rt::TypeName::new(#type_name),
                module_path: #rt::ModulePath::new(::core::module_path!()),
                kind: #kind,
                component: #rt::ComponentName::new(#component),
            })
            .fields(#fields)
            .variants(#variants)
            #property_metadata
            .build();
        };
    };
    Ok(expanded)
}

// Concrete method resolution prefers &Probe (whole Self) and only autoref
// falls back to &&Probe (ignored fields). Neither impl overlaps the other.
fn ignored_defaults(ignored: &[&syn::Field], selected: &[(&Ident, &Ident)]) -> TokenStream2 {
    let probe = hygienic_ident("__SpecgateDefaultProbe");
    let defaults = hygienic_ident("__SpecgateDefaults");
    let supply = hygienic_ident("__SpecgateSupply");
    let method = hygienic_ident("__specgate_supply_defaults");
    let whole = hygienic_ident("__specgate_whole_default");
    let whole_pattern = if selected.is_empty() { quote!(#whole) } else { quote!(mut #whole) };
    let replacements = selected.iter().map(|(ident, local)| quote!(#whole.#ident = #local;));
    let fields = selected.iter().map(|(ident, local)| quote!(#ident: #local));
    let mut tuple = quote!(());
    let mut pattern = quote!(());
    let mut initializers = Vec::new();
    for (index, field) in ignored.iter().enumerate().rev() {
        let ty = &field.ty;
        let ident = &field.ident;
        let local = hygienic_ident(format!("__specgate_ignored_{index}"));
        tuple = quote!((#ty, #tuple));
        pattern = quote!((#local, #pattern));
        initializers.push(quote!(#ident: #local));
    }
    quote!({
        struct #probe<S, F>(::core::marker::PhantomData<(S, F)>);
        enum #defaults<S, F> { Whole(S), Fields(F) }
        trait #supply<S, F> { fn #method(self) -> #defaults<S, F>; }
        impl<S: ::core::default::Default, F> #supply<S, F> for &#probe<S, F> {
            fn #method(self) -> #defaults<S, F> { #defaults::Whole(S::default()) }
        }
        impl<S, F: ::core::default::Default> #supply<S, F> for &&#probe<S, F> {
            fn #method(self) -> #defaults<S, F> { #defaults::Fields(F::default()) }
        }
        match (&#probe::<Self, #tuple>(::core::marker::PhantomData)).#method() {
            #defaults::Whole(#whole_pattern) => { #(#replacements)* #whole },
            #defaults::Fields(#pattern) => Self { #(#fields,)* #(#initializers,)* },
        }
    })
}

struct EventField {
    name: String,
    path: bool,
    ignored: bool,
}

impl EventField {
    fn new(name: String) -> Self {
        Self {
            name,
            path: false,
            ignored: false,
        }
    }
}

fn event_field(attributes: &[syn::Attribute], default: impl Into<String>) -> syn::Result<Option<EventField>> {
    let mut options = EventField::new(default.into());
    let mut annotated = false;
    for attribute in attributes.iter().filter(|attribute| attribute.path().is_ident("spec_event")) {
        annotated = true;
        if matches!(attribute.meta, syn::Meta::Path(_)) {
            continue;
        }
        if let Ok(literal) = attribute.parse_args::<LitStr>() {
            options.name = literal.value();
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                options.name = meta.value()?.parse::<LitStr>()?.value();
            } else if meta.path.is_ident("path") {
                options.path = true;
            } else if meta.path.is_ident("ignore") {
                options.ignored = true;
            } else {
                return Err(meta.error("expected `name`, `path`, or `ignore`"));
            }
            Ok(())
        })?;
    }
    Ok(annotated.then_some(options))
}

fn hygienic_ident(value: impl AsRef<str>) -> Ident {
    Ident::new(value.as_ref(), Span::mixed_site())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_ignore_metadata_rejects_in_every_payload_shape() {
        for field in [
            quote!(#[spec_event(ignore = true)]),
            quote!(#[spec_event(ignore(no))]),
            quote!(#[spec_event(ignroe)]),
        ] {
            for input in [
                quote!(struct Record { #field value: i32 }),
                quote!(enum Named { Case { #field value: i32 } }),
                quote!(enum Tuple { Case(#field i32) }),
            ] {
                expand_event(input).unwrap_err();
            }
        }
    }

    #[test]
    fn property_setup_name_is_independent_of_attribute_order() {
        let first = expand_event(quote! {
            #[spec_event(name = "Canonical", setup)]
            pub struct Native { pub value: u32 }
        })
        .unwrap();
        let last = expand_event(quote! {
            #[spec_event(setup, name = "Canonical")]
            pub struct Native { pub value: u32 }
        })
        .unwrap();
        assert!(last.to_string().contains("\"Canonical\""));
        assert_eq!(first.to_string(), last.to_string());
    }
}
