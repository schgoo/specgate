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
    let type_name = event_name(&input.attrs, name.to_string()).unwrap_or_else(|| name.to_string());

    let (projection, fields, variants, kind) = match &input.data {
        Data::Struct(data) => {
            let Fields::Named(named) = &data.fields else {
                return Err(syn::Error::new_spanned(name, "SpecEvent structs must use named fields"));
            };
            let selected = named
                .named
                .iter()
                .filter_map(|field| {
                    let ident = field.ident.as_ref()?;
                    event_field(&field.attrs, ident.to_string())
                        .map(|(semantic_name, path_projection)| (ident, &field.ty, semantic_name, path_projection))
                })
                .collect::<Vec<_>>();
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
                        let selected = named
                            .named
                            .iter()
                            .enumerate()
                            .filter_map(|(field_index, field)| {
                                let ident = field.ident.as_ref()?;
                                Some((
                                    ident,
                                    hygienic_ident(format!("__specgate_variant_{variant_index}_field_{field_index}")),
                                    &field.ty,
                                    event_name(&field.attrs, ident.to_string()).unwrap_or_else(|| ident.to_string()),
                                ))
                            })
                            .collect::<Vec<_>>();
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
                        let values = bindings.iter().map(|binding| quote!(#rt::ToNativeValue::to_native_value(#binding)));
                        let tuple = unnamed.unnamed.iter().map(|field| {
                            let ty = &field.ty;
                            let ty = quote!(#ty).to_string();
                            quote!(#rt::RustType::new(#ty))
                        });
                        arms.push(quote! {
                            Self::#variant_ident(#(#bindings),*) => #rt::Value::Map(::std::collections::BTreeMap::from([(
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
    let expanded = quote! {
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
            .build();
        };
    };
    Ok(expanded)
}

fn event_field(attributes: impl AsRef<[syn::Attribute]>, default: impl AsRef<str>) -> Option<(String, bool)> {
    let attributes = attributes.as_ref();
    let default = default.as_ref();
    let attribute = attributes.iter().find(|attribute| attribute.path().is_ident("spec_event"))?;
    if attribute.meta.require_list().is_err() {
        return Some((default.to_string(), false));
    }
    if let Ok(literal) = attribute.parse_args::<LitStr>() {
        return Some((literal.value(), false));
    }
    let mut name = None;
    let mut path_projection = false;
    let _ = attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("name") {
            name = Some(meta.value()?.parse::<LitStr>()?.value());
            Ok(())
        } else if meta.path.is_ident("path") {
            path_projection = true;
            Ok(())
        } else {
            Err(meta.error("expected `name` or `path`"))
        }
    });
    Some((name.unwrap_or_else(|| default.to_string()), path_projection))
}

fn event_name(attributes: impl AsRef<[syn::Attribute]>, default: impl AsRef<str>) -> Option<String> {
    event_field(attributes, default).map(|(name, _path_projection)| name)
}

fn hygienic_ident(value: impl AsRef<str>) -> Ident {
    Ident::new(value.as_ref(), Span::mixed_site())
}
