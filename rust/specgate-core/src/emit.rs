use crate::types::*;

/// Maps a language-native type name to a spec type name.
/// Front-ends report types in their native syntax; this function normalizes them.
fn map_type(source_language: &str, native_type: &str) -> String {
    match source_language {
        "csharp" => map_csharp_type(native_type),
        "rust" => map_rust_type(native_type),
        _ => native_type.to_string(),
    }
}

fn map_csharp_type(t: &str) -> String {
    // Unwrap Task<T> and ValueTask<T>
    let t = strip_wrapper(t, "Task<", ">");
    let t = strip_wrapper(&t, "ValueTask<", ">");

    match t.as_str() {
        "string" | "String" | "System.String" => "str".to_string(),
        "int" | "Int32" | "System.Int32" => "int".to_string(),
        "long" | "Int64" | "System.Int64" => "int64".to_string(),
        "bool" | "Boolean" | "System.Boolean" => "bool".to_string(),
        "double" | "Double" | "System.Double" => "float64".to_string(),
        "float" | "Single" | "System.Single" => "float32".to_string(),
        "Guid" | "System.Guid" => "uuid".to_string(),
        "void" | "System.Void" => "void".to_string(),
        _ => {
            // Handle Nullable<T> → Option[T]
            if let Some(inner) = try_strip_wrapper(&t, "Nullable<", ">") {
                return format!("Option[{}]", map_csharp_type(&inner));
            }
            // Handle T? syntax
            if t.ends_with('?') {
                let inner = &t[..t.len() - 1];
                return format!("Option[{}]", map_csharp_type(inner));
            }
            // Handle List<T>, IList<T>, IReadOnlyList<T>
            for prefix in ["List<", "IList<", "IReadOnlyList<", "IEnumerable<"] {
                if let Some(inner) = try_strip_wrapper(&t, prefix, ">") {
                    return format!("List[{}]", map_csharp_type(&inner));
                }
            }
            // Handle Dictionary<K,V>
            for prefix in ["Dictionary<", "IDictionary<", "IReadOnlyDictionary<"] {
                if let Some(inner) = try_strip_wrapper(&t, prefix, ">") {
                    if let Some((k, v)) = inner.split_once(',') {
                        return format!(
                            "Map[{}, {}]",
                            map_csharp_type(k.trim()),
                            map_csharp_type(v.trim())
                        );
                    }
                }
            }
            // Strip namespace, keep last segment
            t.rsplit('.').next().unwrap_or(&t).to_string()
        }
    }
}

fn map_rust_type(t: &str) -> String {
    match t {
        "String" | "&str" => "str".to_string(),
        "i32" | "i64" | "u32" | "u64" | "usize" | "isize" => t.to_string(),
        "bool" => "bool".to_string(),
        "f32" => "float32".to_string(),
        "f64" => "float64".to_string(),
        _ => {
            if let Some(inner) = try_strip_wrapper(t, "Option<", ">") {
                return format!("Option[{}]", map_rust_type(&inner));
            }
            if let Some(inner) = try_strip_wrapper(t, "Vec<", ">") {
                return format!("List[{}]", map_rust_type(&inner));
            }
            if let Some(inner) = try_strip_wrapper(t, "HashMap<", ">") {
                if let Some((k, v)) = inner.split_once(',') {
                    return format!(
                        "Map[{}, {}]",
                        map_rust_type(k.trim()),
                        map_rust_type(v.trim())
                    );
                }
            }
            // Strip module path
            t.rsplit("::").next().unwrap_or(t).to_string()
        }
    }
}

fn strip_wrapper(s: &str, prefix: &str, suffix: &str) -> String {
    try_strip_wrapper(s, prefix, suffix).unwrap_or_else(|| s.to_string())
}

fn try_strip_wrapper(s: &str, prefix: &str, suffix: &str) -> Option<String> {
    let s = s.trim();
    if s.starts_with(prefix) && s.ends_with(suffix) {
        Some(s[prefix.len()..s.len() - suffix.len()].to_string())
    } else {
        None
    }
}

pub fn emit_specs(operations: &[ValidatedOperation], source_language: &str) -> Vec<SpecFile> {
    operations
        .iter()
        .map(|op| emit_one(op, source_language))
        .collect()
}

fn emit_one(op: &ValidatedOperation, lang: &str) -> SpecFile {
    SpecFile {
        name: op.name.clone(),
        kind: op.kind,
        inputs: op
            .inputs
            .iter()
            .map(|f| TypedField {
                name: f.name.clone(),
                type_name: map_type(lang, &f.type_name),
            })
            .collect(),
        environments: op
            .environments
            .iter()
            .map(|f| TypedField {
                name: f.name.clone(),
                type_name: map_type(lang, &f.type_name),
            })
            .collect(),
        dependencies: op
            .dependencies
            .iter()
            .map(|d| DependencyField {
                name: d.name.clone(),
                type_name: map_type(lang, &d.type_name),
                dep: d.dep.clone(),
            })
            .collect(),
        checkpoints: op
            .checkpoints
            .iter()
            .map(|f| TypedField {
                name: f.name.clone(),
                type_name: map_type(lang, &f.type_name),
            })
            .collect(),
        states: op
            .states
            .iter()
            .map(|f| TypedField {
                name: f.name.clone(),
                type_name: map_type(lang, &f.type_name),
            })
            .collect(),
        outcome: None,   // TODO: extract from return type analysis
        outputs: None,    // TODO: extract from return type analysis
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csharp_primitive_mapping() {
        assert_eq!(map_csharp_type("string"), "str");
        assert_eq!(map_csharp_type("int"), "int");
        assert_eq!(map_csharp_type("bool"), "bool");
        assert_eq!(map_csharp_type("Guid"), "uuid");
    }

    #[test]
    fn csharp_task_unwrap() {
        assert_eq!(map_csharp_type("Task<string>"), "str");
        assert_eq!(map_csharp_type("Task<List<int>>"), "List[int]");
    }

    #[test]
    fn csharp_nullable() {
        assert_eq!(map_csharp_type("string?"), "Option[str]");
        assert_eq!(map_csharp_type("Nullable<int>"), "Option[int]");
    }

    #[test]
    fn csharp_collections() {
        assert_eq!(map_csharp_type("List<string>"), "List[str]");
        assert_eq!(map_csharp_type("Dictionary<string, int>"), "Map[str, int]");
    }

    #[test]
    fn csharp_namespace_strip() {
        assert_eq!(map_csharp_type("Microsoft.Graph.User"), "User");
    }

    #[test]
    fn rust_type_mapping() {
        assert_eq!(map_rust_type("String"), "str");
        assert_eq!(map_rust_type("Option<String>"), "Option[str]");
        assert_eq!(map_rust_type("Vec<i32>"), "List[i32]");
    }
}
