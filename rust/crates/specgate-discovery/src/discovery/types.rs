//! Native type parsing and semantic mapping.
//!
//! # Example
//! ```no_run
//! use specgate_discovery::types::map;
//! assert_eq!(map("String", [])?.ref_string(), "string");
//! # Ok::<(), specgate_discovery::Error>(())
//! ```

// ---------------------------------------------------------------------------
// Type-ref mapping
// ---------------------------------------------------------------------------

/// A mapped semantic type reference.
///
/// Recursive storage is encapsulated; callers construct values through mapping
/// functions and inspect their stable single-line representation.
///
/// # Examples
///
/// ```
/// use specgate_discovery::types::map;
/// assert_eq!(map("String", [])?.ref_string(), "string");
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecType(TypeRepr);

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeRepr {
    Scalar(String),
    Map { keys: Box<SpecType>, values: Box<SpecType> },
    Set { items: Box<SpecType> },
}

impl SpecType {
    fn scalar(value: impl Into<String>) -> Self {
        Self(TypeRepr::Scalar(value.into()))
    }
    fn map(keys: SpecType, values: SpecType) -> Self {
        Self(TypeRepr::Map {
            keys: Box::new(keys),
            values: Box::new(values),
        })
    }
    fn set(items: SpecType) -> Self {
        Self(TypeRepr::Set { items: Box::new(items) })
    }
    /// Render as a single-line reference string used by registry normalization.
    #[must_use]
    pub fn ref_string(&self) -> String {
        match &self.0 {
            TypeRepr::Scalar(value) => value.clone(),
            TypeRepr::Map { keys, values } => format!("map<{}, {}>", keys.ref_string(), values.ref_string()),
            TypeRepr::Set { items } => format!("set<{}>", items.ref_string()),
        }
    }
}

/// Parsed Rust type AST for semantic normalization.
///
/// # Examples
///
/// ```
/// use specgate_discovery::types::map;
/// let value = map("Option<Vec<i32>>", [])?;
/// assert_eq!(value.ref_string(), "Option<List<i32>>");
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RustType {
    /// A named type with zero or more generic arguments.
    Named {
        /// Final path segment naming the type.
        name: String,
        /// Parsed generic type arguments.
        args: Vec<RustType>,
    },
    /// A shared or mutable reference to another type.
    Ref(Box<RustType>),
    /// A dynamically sized slice type.
    Slice(Box<RustType>),
}

/// Map a stringified Rust type to its semantic type reference.
///
/// The input uses `quote!(#ty).to_string()` formatting, which inserts spaces
/// around tokens. Mapping recurses into generic arguments. `type_names` are
/// the registered `SpecEvent` types passed through by bare name.
/// Parse and map a stringified Rust type to its semantic representation.
///
/// # Errors
/// Returns a normalization error when the complete Rust type expression cannot be parsed.
pub fn map<'a>(ty: impl AsRef<str>, type_names: impl AsRef<[&'a str]>) -> Result<SpecType, crate::error::Error> {
    let ty = ty.as_ref();
    let type_names = type_names.as_ref();
    let parsed = parse(ty).ok_or_else(|| {
        crate::error::Error::message(
            crate::error::ErrorKind::Normalization,
            format!("invalid Rust type expression '{ty}'"),
        )
    })?;
    Ok(map_rust(&parsed, type_names))
}

fn map_rust(t: &RustType, type_names: &[&str]) -> SpecType {
    match t {
        RustType::Ref(inner) => map_rust(inner, type_names),
        RustType::Slice(inner) => SpecType::scalar(format!("List<{}>", map_rust(inner, type_names).ref_string())),
        RustType::Named { name, args } => map_named(name, args, type_names),
    }
}

fn map_named(name: &str, args: &[RustType], type_names: &[&str]) -> SpecType {
    let m = |t: &RustType| map_rust(t, type_names);
    match (name, args.len()) {
        ("String" | "str" | "ComponentId" | "OperationName" | "TargetName", _) => SpecType::scalar("string".to_string()),
        ("()", 0) => SpecType::scalar("unit".to_string()),
        // The runtime `Value` is the spec's built-in universal structured value.
        (_, 0) if runtime_value(name) => SpecType::scalar("value".to_string()),
        ("Option", 1) => SpecType::scalar(format!("Option<{}>", m(&args[0]).ref_string())),
        ("Vec", 1) => SpecType::scalar(format!("List<{}>", m(&args[0]).ref_string())),
        ("Result", 2) => SpecType::scalar(format!("Result<{}, {}>", m(&args[0]).ref_string(), m(&args[1]).ref_string())),
        ("HashMap" | "BTreeMap", 2) => SpecType::map(m(&args[0]), m(&args[1])),
        ("HashSet" | "BTreeSet", 1) => SpecType::set(m(&args[0])),
        // Named SpecEvent type or any other bare name: pass through by name.
        _ => SpecType::scalar(name.to_string()),
    }
}

/// True for primitive scalars and the collection/option/result constructors
/// normalization maps directly, rather than as named `SpecEvent` references.
///
/// # Examples
///
/// ```
/// assert!(specgate_discovery::types::builtin("Vec"));
/// assert!(!specgate_discovery::types::builtin("Order"));
/// ```
#[must_use]
pub fn builtin(name: impl AsRef<str>) -> bool {
    let name = name.as_ref();
    runtime_value(name)
        || matches!(
            name,
            "String"
                | "string"
                | "str"
                | "ComponentId"
                | "OperationName"
                | "TargetName"
                | "char"
                | "bool"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "Option"
                | "optional"
                | "Vec"
                | "List"
                | "list"
                | "Result"
                | "HashMap"
                | "BTreeMap"
                | "Map"
                | "map"
                | "HashSet"
                | "BTreeSet"
                | "Set"
                | "set"
                | "()"
                | "unit"
                | "value"
        )
}

/// Test whether a name denotes the runtime semantic `Value` type.
///
/// # Examples
///
/// ```
/// assert!(specgate_discovery::types::runtime_value("specgate::Value"));
/// ```
///
/// Type-path parsing keeps only the last path segment, so any of `Value`,
/// `specgate_runtime::value::Value`, `::specgate_runtime::value::Value`, or
/// `specgate::Value`
/// arrive here as the bare name `Value`. It maps to the built-in `value` spec
/// type.
#[must_use]
pub fn runtime_value(name: impl AsRef<str>) -> bool {
    let name = name.as_ref();
    matches!(
        name,
        "Value"
            | "specgate_runtime::Value"
            | "::specgate_runtime::Value"
            | "specgate_runtime::value::Value"
            | "::specgate_runtime::value::Value"
            | "specgate::Value"
    )
}

/// Collect every non-builtin named type referenced inside a stringified Rust
/// type (recursing into generic args), in source order.
///
/// # Examples
///
/// ```
/// assert_eq!(specgate_discovery::types::collect_refs("Option<Order>"), ["Order"]);
/// ```
#[must_use]
pub fn collect_refs(ty: impl AsRef<str>) -> Vec<String> {
    let mut references = Vec::new();
    collect_into(ty, &mut references);
    references
}

/// Append named references from a Rust type spelling into reusable storage.
pub(crate) fn collect_into(ty: impl AsRef<str>, references: &mut Vec<String>) {
    if let Some(parsed) = parse(ty.as_ref()) {
        append_refs(&parsed, references);
    }
}

fn append_refs(t: &RustType, out: &mut Vec<String>) {
    match t {
        RustType::Ref(inner) | RustType::Slice(inner) => append_refs(inner, out),
        RustType::Named { name, args } => {
            if !builtin(name) {
                out.push(name.clone());
            }
            for a in args {
                append_refs(a, out);
            }
        }
    }
}

/// Parse a (possibly space-separated) Rust type string into a [`RustType`].
pub(super) fn parse(s: &str) -> Option<RustType> {
    let tokens = tokenize_type(s);
    let mut pos = 0;
    let t = parse_tokens(&tokens, &mut pos)?;
    (pos == tokens.len()).then_some(t)
}

fn tokenize_type(s: &str) -> Vec<String> {
    let mut tokens = Vec::with_capacity(s.len());
    let mut cur = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' | '>' | ',' | '&' | '[' | ']' | '(' | ')' => {
                if !cur.trim().is_empty() {
                    tokens.push(cur.trim().to_string());
                }
                cur.clear();
                tokens.push(c.to_string());
            }
            c if c.is_whitespace() => {
                if !cur.trim().is_empty() {
                    tokens.push(cur.trim().to_string());
                }
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        tokens.push(cur.trim().to_string());
    }
    tokens
}

fn parse_tokens(tokens: &[String], pos: &mut usize) -> Option<RustType> {
    let tok = tokens.get(*pos)?.as_str();
    match tok {
        "&" => {
            *pos += 1;
            // Skip a lifetime token (e.g. `'a`) if present.
            if tokens.get(*pos).is_some_and(|t| t.starts_with('\'')) {
                *pos += 1;
            }
            if tokens.get(*pos).map(String::as_str) == Some("mut") {
                *pos += 1;
            }
            let inner = parse_tokens(tokens, pos)?;
            Some(RustType::Ref(Box::new(inner)))
        }
        "[" => {
            *pos += 1;
            let inner = parse_tokens(tokens, pos)?;
            if tokens.get(*pos).map(String::as_str) != Some("]") {
                return None;
            }
            *pos += 1;
            Some(RustType::Slice(Box::new(inner)))
        }
        "(" => {
            if tokens.get(*pos + 1).map(String::as_str) != Some(")") {
                return None;
            }
            *pos += 2;
            Some(RustType::Named {
                name: "()".to_string(),
                args: Vec::new(),
            })
        }
        _ => {
            // Skip a leading path separator (`::Foo` / `::specgate_runtime::value::Value`).
            if tok == "::" {
                *pos += 1;
            }
            // A path like `std :: collections :: BTreeMap` - keep the last segment.
            let mut name = tokens.get(*pos)?.clone();
            *pos += 1;
            while tokens.get(*pos).map(String::as_str) == Some("::") {
                *pos += 1;
                if let Some(seg) = tokens.get(*pos) {
                    name.clone_from(seg);
                    *pos += 1;
                }
            }
            // Strip `::` if the tokenizer kept colons inside the segment.
            if let Some(idx) = name.rfind("::") {
                name = name[idx + 2..].to_string();
            }
            let mut args = Vec::new();
            if tokens.get(*pos).map(String::as_str) == Some("<") {
                *pos += 1;
                loop {
                    if *pos >= tokens.len() {
                        return None;
                    }
                    if tokens.get(*pos).map(String::as_str) == Some(">") {
                        return None;
                    }
                    let arg = parse_tokens(tokens, pos)?;
                    args.push(arg);
                    match tokens.get(*pos).map(String::as_str) {
                        Some(",") => {
                            *pos += 1;
                        }
                        Some(">") => {
                            *pos += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
            }
            Some(RustType::Named { name, args })
        }
    }
}

/// Normalize a type string for equality comparison by collapsing every whitespace run.
///
/// Leading and trailing whitespace is removed. Punctuation is preserved, so this helper does
/// not parse or otherwise reinterpret malformed type spellings.
///
/// # Examples
/// ```
/// assert_eq!(specgate_discovery::types::normalize("  Result < i32,   Error > "), "Result < i32, Error >");
/// ```
#[must_use]
pub fn normalize(value: impl AsRef<str>) -> String {
    value.as_ref().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Return whether a type spelling represents the unit return type.
///
/// Both an empty or whitespace-only spelling and the explicit `()` spelling are unit. Other
/// tuple spellings are not.
///
/// # Examples
/// ```
/// assert!(specgate_discovery::types::is_unit("  "));
/// assert!(specgate_discovery::types::is_unit("()"));
/// assert!(!specgate_discovery::types::is_unit("(i32,)"));
/// ```
#[must_use]
pub fn is_unit(ty: impl AsRef<str>) -> bool {
    let n = normalize(ty);
    n.is_empty() || n == "()"
}

#[cfg(test)]
mod tests {
    use super::{collect_refs, map, parse};

    #[test]
    fn rejects_malformed_types() {
        for value in ["Vec<", "&", "[Order", "Map<String,,Order>"] {
            assert!(parse(value).is_none(), "parsed malformed type: {value}");
            assert!(map(value, []).is_err(), "mapped malformed type: {value}");
        }
    }

    #[test]
    fn normalizes_references() {
        assert_eq!(map("&'a [crate::model::Order]", ["Order"]).unwrap().ref_string(), "List<Order>");
        assert_eq!(collect_refs("&'a Option<crate::model::Order>"), ["Order"]);
    }

    #[test]
    fn preserves_collections() {
        assert_eq!(
            map("std::collections::BTreeMap<String, Vec<Order>>", ["Order"])
                .unwrap()
                .ref_string(),
            "map<string, List<Order>>"
        );
        assert_eq!(
            map("HashSet<Result<Order, Error>>", ["Order", "Error"]).unwrap().ref_string(),
            "set<Result<Order, Error>>"
        );
    }
}
