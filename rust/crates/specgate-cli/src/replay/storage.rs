//! Owned Rust input forms accepted by the registry-guided replay planner.

use super::{Error, ReplayType, failure};

#[derive(Clone, Copy)]
pub(super) struct ValueContext<'a> {
    pub(super) component: &'a str,
    pub(super) operation: &'a str,
    pub(super) name: &'a str,
}

impl<'a> ValueContext<'a> {
    pub(super) fn new(component: &'a str, operation: &'a str, name: &'a str) -> Self {
        Self {
            component,
            operation,
            name,
        }
    }
}

pub(super) fn check_rust(semantic: &ReplayType, rust_type: &str, context: ValueContext<'_>) -> Result<(), Error> {
    let ValueContext {
        component,
        operation,
        name,
    } = context;
    let parsed: syn::Type = syn::parse_str(rust_type).map_err(|error| format!("invalid Rust replay input type '{rust_type}': {error}"))?;
    let native = rust_type.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    let supported = if let Some(primitive) = semantic.primitive_name() {
        match primitive {
            "unit" => native == "()",
            "string" => native == "String" || native == "&str",
            "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" => primitive == native,
            _ => false,
        }
    } else if let Some(items) = semantic.tuple_items() {
        let arguments = tuple_arguments(&parsed);
        if let Some(arguments) = arguments.filter(|arguments| arguments.len() == items.len() && (2..=4).contains(&items.len())) {
            if arguments.iter().any(|argument| argument.contains('&')) {
                return failure(format!(
                    "candidate operation '{component}::{operation}' input '{name}' uses borrowed tuple fields; replay requires owned fields"
                ));
            }
            for (item, argument) in items.iter().zip(arguments) {
                check_rust(item, &argument, context)?;
            }
            true
        } else {
            false
        }
    } else if semantic.named_parts().is_some() || semantic.record_fields().is_some() {
        return failure(format!(
            "operation '{component}::{operation}' value '{name}' uses unsupported structured type"
        ));
    } else {
        let (constructor, arguments) = native_arguments(rust_type)?;
        if arguments.iter().any(|argument| argument.contains('&')) {
            return failure(format!(
                "candidate operation '{component}::{operation}' input '{name}' uses borrowed collection elements; replay requires owned elements"
            ));
        }
        if let Some(item) = semantic.element_type() {
            let accepted = constructor == "Vec"
                || (semantic.ordered() == Some(false)
                    && semantic.unique() == Some(true)
                    && matches!(constructor.as_str(), "HashSet" | "BTreeSet"));
            if accepted && arguments.len() == 1 {
                check_rust(item, &arguments[0], context)?;
                true
            } else {
                false
            }
        } else if let Some(inner) = semantic.optional_value() {
            if constructor == "Option" && arguments.len() == 1 {
                check_rust(inner, &arguments[0], context)?;
                true
            } else {
                false
            }
        } else if let Some((keys, values)) = semantic.map_types() {
            if constructor == "Vec" && arguments.len() == 1 {
                let pair = syn::parse_str::<syn::Type>(&arguments[0]).expect("parsed generic type");
                if let Some(pair) = tuple_arguments(&pair).filter(|pair| pair.len() == 2) {
                    check_rust(keys, &pair[0], context)?;
                    check_rust(values, &pair[1], context)?;
                    true
                } else {
                    false
                }
            } else if semantic.ordered() == Some(false)
                && keys.primitive_name() == Some("string")
                && matches!(constructor.as_str(), "HashMap" | "BTreeMap")
                && arguments.len() == 2
            {
                check_rust(keys, &arguments[0], context)?;
                check_rust(values, &arguments[1], context)?;
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if supported {
        Ok(())
    } else {
        failure(format!(
            "candidate operation '{component}::{operation}' input '{name}' uses unsupported Rust type '{rust_type}' for registry type {semantic:?}"
        ))
    }
}

pub(super) fn tuple_arguments(ty: &syn::Type) -> Option<Vec<String>> {
    let syn::Type::Tuple(tuple) = ty else { return None };
    Some(tuple.elems.iter().map(|ty| quote::quote!(#ty).to_string()).collect())
}

pub(super) fn native_arguments(rust_type: &str) -> Result<(String, Vec<String>), Error> {
    let parsed: syn::Type = syn::parse_str(rust_type).map_err(|error| format!("invalid Rust replay input type '{rust_type}': {error}"))?;
    let syn::Type::Path(path) = parsed else {
        return failure(format!("unsupported Rust collection input type '{rust_type}'"));
    };
    let segment = path.path.segments.last().ok_or_else(|| Error::from("missing collection type"))?;
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return failure(format!("missing Rust collection type arguments for '{rust_type}'"));
    };
    let arguments = arguments
        .args
        .iter()
        .map(|argument| match argument {
            syn::GenericArgument::Type(ty) => Ok(quote::quote!(#ty).to_string()),
            _ => failure(format!("unsupported Rust collection type argument in '{rust_type}'")),
        })
        .collect::<Result<_, _>>()?;
    Ok((segment.ident.to_string(), arguments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_checks_dimensions_keys_and_owned_nested_fields() {
        let integer = ReplayType::primitive("i32");
        let ordered_map = serde_json::from_value(serde_json::json!({
            "kind":"map", "ordered":true,
            "keys":{"kind":"primitive","name":"string"},
            "values":{"kind":"primitive","name":"i32"}
        }))
        .unwrap();
        for (semantic, accepted, rejected) in [
            (ReplayType::collection(integer.clone(), false, false), "Vec<i32>", "BTreeSet<i32>"),
            (ReplayType::collection(integer.clone(), true, true), "Vec<i32>", "HashSet<i32>"),
            (ordered_map, "Vec<(String, i32)>", "BTreeMap<String, i32>"),
            (
                ReplayType::map(integer, ReplayType::primitive("string")),
                "Vec<(i32, String)>",
                "BTreeMap<i32, String>",
            ),
            (
                ReplayType::tuple(vec![
                    ReplayType::primitive("string"),
                    ReplayType::list(ReplayType::primitive("i32")),
                ]),
                "(String, Vec<i32>)",
                "(&str, Vec<i32>)",
            ),
        ] {
            let context = ValueContext::new("fixture.storage", "store", "value");
            check_rust(&semantic, accepted, context).unwrap();
            let error = check_rust(&semantic, rejected, context).unwrap_err();
            assert!(error.to_string().contains("fixture.storage::store"), "{error}");
        }
    }
}
