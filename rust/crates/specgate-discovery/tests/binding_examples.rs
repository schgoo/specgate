//! Documented and checked-in bindings must match `binding-schema.json` and the
//! real binding loader.
//!
//! A documentation example opts in by making its first line inside a YAML code
//! fence `# yaml-language-server: $schema=<relative path>/binding-schema.json`.
//! Outside the normative `docs/ctsc` contracts and the immutable
//! `docs/decisions` records, a YAML block shaped like a binding (a top-level
//! `components:` key) must carry that marker, so an example cannot silently
//! escape the check. The schema is applied by a deliberately small draft-07
//! evaluator that fails on any keyword it does not implement.

use serde_json::Value;
use specgate_discovery::binding::parse_binding;
use std::path::{Path, PathBuf};

const MARKER: &str = "# yaml-language-server: $schema=";
const SCHEMA_FILE: &str = "binding-schema.json";
const SKIPPED_DIRS: &[&str] = &["target", "bin", "obj", ".git", "node_modules", "artifacts", ".specgate"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn load_schema(root: &Path) -> Value {
    let text = std::fs::read_to_string(root.join(SCHEMA_FILE)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn validate(schema: &Value, root: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    let Value::Object(keywords) = schema else {
        panic!("schema at {at} is not an object");
    };
    for (keyword, rule) in keywords {
        match keyword.as_str() {
            "$schema" | "title" | "description" | "definitions" | "then" | "else" => {}
            "$ref" => {
                let pointer = rule.as_str().unwrap().strip_prefix('#').expect("only local $ref is supported");
                let target = root.pointer(pointer).unwrap_or_else(|| panic!("unresolved $ref {pointer}"));
                validate(target, root, value, at, errors);
            }
            "type" => {
                let matches = match rule.as_str().unwrap() {
                    "object" => value.is_object(),
                    "string" => value.is_string(),
                    "array" => value.is_array(),
                    other => panic!("unsupported schema type {other}"),
                };
                if !matches {
                    errors.push(format!("{at}: expected {rule}"));
                }
            }
            "required" => {
                if let Value::Object(object) = value {
                    for name in rule.as_array().unwrap() {
                        let name = name.as_str().unwrap();
                        if !object.contains_key(name) {
                            errors.push(format!("{at}: missing required '{name}'"));
                        }
                    }
                }
            }
            "properties" => {
                if let Value::Object(object) = value {
                    for (name, property) in rule.as_object().unwrap() {
                        if let Some(child) = object.get(name) {
                            validate(property, root, child, &format!("{at}/{name}"), errors);
                        }
                    }
                }
            }
            "additionalProperties" => {
                if let Value::Object(object) = value {
                    let declared = keywords.get("properties").and_then(Value::as_object);
                    for (name, child) in object {
                        if declared.is_some_and(|properties| properties.contains_key(name)) {
                            continue;
                        }
                        match rule {
                            Value::Bool(false) => errors.push(format!("{at}: unknown property '{name}'")),
                            Value::Bool(true) => {}
                            nested => validate(nested, root, child, &format!("{at}/{name}"), errors),
                        }
                    }
                }
            }
            "minProperties" => {
                if let Value::Object(object) = value
                    && (object.len() as u64) < rule.as_u64().unwrap()
                {
                    errors.push(format!("{at}: needs at least {rule} properties"));
                }
            }
            "propertyNames" => {
                if let Value::Object(object) = value {
                    for name in object.keys() {
                        validate(rule, root, &Value::String(name.clone()), &format!("{at}/<{name}>"), errors);
                    }
                }
            }
            "enum" => {
                if !rule.as_array().unwrap().contains(value) {
                    errors.push(format!("{at}: {value} is not one of {rule}"));
                }
            }
            "const" => {
                if rule != value {
                    errors.push(format!("{at}: {value} is not {rule}"));
                }
            }
            "minLength" => {
                if let Value::String(text) = value
                    && (text.chars().count() as u64) < rule.as_u64().unwrap()
                {
                    errors.push(format!("{at}: shorter than {rule}"));
                }
            }
            "pattern" => {
                assert_eq!(rule.as_str(), Some("\\S"), "unsupported schema pattern {rule}");
                if let Value::String(text) = value
                    && text.trim().is_empty()
                {
                    errors.push(format!("{at}: must contain a non-whitespace character"));
                }
            }
            "minItems" => {
                if let Value::Array(items) = value
                    && (items.len() as u64) < rule.as_u64().unwrap()
                {
                    errors.push(format!("{at}: needs at least {rule} items"));
                }
            }
            "items" => {
                if let Value::Array(items) = value {
                    for (index, item) in items.iter().enumerate() {
                        validate(rule, root, item, &format!("{at}/{index}"), errors);
                    }
                }
            }
            "allOf" => {
                for nested in rule.as_array().unwrap() {
                    validate(nested, root, value, at, errors);
                }
            }
            "if" => {
                let branch = if passes(rule, root, value, at) { "then" } else { "else" };
                if let Some(nested) = keywords.get(branch) {
                    validate(nested, root, value, at, errors);
                }
            }
            "not" => {
                if passes(rule, root, value, at) {
                    errors.push(format!("{at}: matches a forbidden shape {rule}"));
                }
            }
            "oneOf" => {
                let matching = rule
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|nested| passes(nested, root, value, at))
                    .count();
                if matching != 1 {
                    errors.push(format!("{at}: matches {matching} of the oneOf alternatives, expected exactly 1"));
                }
            }
            other => panic!("binding_examples does not implement schema keyword '{other}'; extend the evaluator"),
        }
    }
}

fn passes(schema: &Value, root: &Value, value: &Value, at: &str) -> bool {
    let mut errors = Vec::new();
    validate(schema, root, value, at, &mut errors);
    errors.is_empty()
}

fn schema_errors(schema: &Value, yaml: &str) -> Vec<String> {
    let value: Value = match serde_yaml::from_str(yaml) {
        Ok(value) => value,
        Err(error) => return vec![format!("YAML: {error}")],
    };
    let mut errors = Vec::new();
    validate(schema, schema, &value, "", &mut errors);
    errors
}

/// A fenced YAML block: its first content line and its body.
#[derive(Debug)]
struct YamlBlock {
    line: usize,
    body: String,
}

fn yaml_blocks(markdown: &str) -> Vec<YamlBlock> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        let fence = line.trim_start();
        if !fence.starts_with("```") {
            continue;
        }
        let info = fence.trim_start_matches('`').trim();
        let mut body = String::new();
        for (_, inner) in lines.by_ref() {
            if inner.trim_start().starts_with("```") {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        if info == "yaml" || info == "yml" {
            blocks.push(YamlBlock { line: index + 2, body });
        }
    }
    blocks
}

/// The `$schema` path declared by a marked block, if it carries the marker.
fn marked_schema(block: &YamlBlock) -> Option<&str> {
    block
        .body
        .lines()
        .next()
        .and_then(|first| first.trim().strip_prefix(MARKER))
        .filter(|path| path.ends_with(SCHEMA_FILE))
}

fn looks_like_binding(block: &YamlBlock) -> bool {
    block.body.lines().any(|line| line.starts_with("components:"))
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for name in ["README.md", "CONTRIBUTING.md", "AGENTS.md", "SUPPORT.md", "CHANGELOG.md"] {
        files.push(root.join(name));
    }
    for directory in ["docs", ".github", "test/behavior", "csharp"] {
        walk(&root.join(directory), &mut |path| {
            if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path.to_path_buf());
            }
        });
    }
    files
}

fn binding_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for directory in ["test", "rust/crates", "docs"] {
        walk(&root.join(directory), &mut |path| {
            let name = path.file_name().unwrap().to_string_lossy();
            if name.starts_with("specgate") && (name.ends_with(".yaml") || name.ends_with(".yml")) {
                files.push(path.to_path_buf());
            }
        });
    }
    files
}

fn walk(directory: &Path, visit: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_string_lossy();
            if !SKIPPED_DIRS.contains(&name.as_ref()) {
                walk(&path, visit);
            }
        } else {
            visit(&path);
        }
    }
}

fn is_exempt(root: &Path, path: &Path) -> bool {
    let docs = root.join("docs");
    path.starts_with(docs.join("ctsc")) || path.starts_with(docs.join("decisions"))
}

/// Check one binding text with both the schema and the real loader.
fn binding_problems(schema: &Value, path: &Path, yaml: &str) -> Vec<String> {
    let mut problems = schema_errors(schema, yaml);
    if let Err(error) = parse_binding(path, yaml) {
        problems.push(format!("loader: {error}"));
    }
    problems
}

/// Problems for every documentation example in one Markdown text.
fn markdown_problems(root: &Path, schema: &Value, path: &Path, markdown: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let directory = path.parent().unwrap();
    for block in yaml_blocks(markdown) {
        let location = format!("{}:{}", path.display(), block.line);
        let Some(schema_path) = marked_schema(&block) else {
            if looks_like_binding(&block) && !is_exempt(root, path) {
                problems.push(format!(
                    "{location}: binding-shaped YAML must start with `{MARKER}<relative path>/{SCHEMA_FILE}`"
                ));
            }
            continue;
        };
        if !directory.join(schema_path).is_file() {
            problems.push(format!(
                "{location}: $schema path '{schema_path}' does not resolve to {SCHEMA_FILE}"
            ));
        }
        for problem in binding_problems(schema, &directory.join("specgate.yaml"), &block.body) {
            problems.push(format!("{location}: {problem}"));
        }
    }
    problems
}

#[test]
fn documented_binding_examples_match_schema_and_loader() {
    let root = repo_root();
    let schema = load_schema(&root);
    let mut problems = Vec::new();
    let mut checked = 0;
    for path in markdown_files(&root) {
        let markdown = std::fs::read_to_string(&path).unwrap();
        checked += yaml_blocks(&markdown).iter().filter(|block| marked_schema(block).is_some()).count();
        problems.extend(markdown_problems(&root, &schema, &path, &markdown));
    }
    assert!(problems.is_empty(), "binding examples are stale:\n{}", problems.join("\n"));
    assert!(
        checked >= 4,
        "expected the guides and reference to carry marked binding examples, found {checked}"
    );
}

#[test]
fn checked_in_bindings_match_schema_and_loader() {
    let root = repo_root();
    let schema = load_schema(&root);
    let files = binding_files(&root);
    assert!(
        files.iter().any(|path| path.ends_with("test/behavior/specgate.yaml")),
        "binding discovery missed test/behavior/specgate.yaml: {files:?}"
    );
    let mut problems = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for problem in binding_problems(&schema, path, &text) {
            problems.push(format!("{}: {problem}", path.display()));
        }
    }
    assert!(problems.is_empty(), "checked-in bindings are invalid:\n{}", problems.join("\n"));
    assert!(files.len() >= 6, "expected at least six checked-in bindings, found {}", files.len());
}

const VALID: &str = "components:\n  example.math:\n    implementations:\n      rust:\n        language: rust\n        sources: crates/math\n        tests:\n          projects:\n            - crates/math/Cargo.toml\n";

#[test]
fn binding_check_rejects_bad_examples() {
    let root = repo_root();
    let schema = load_schema(&root);
    let path = root.join("docs/guides/specgate.yaml");
    assert_eq!(binding_problems(&schema, &path, VALID), Vec::<String>::new());

    let invalid = [
        (
            "unknown field",
            VALID.replace("        sources:", "        source_dir: x\n        sources:"),
        ),
        (
            "wrong language block",
            format!("{VALID}        csharp:\n          framework: net10.0\n"),
        ),
        ("unknown language", VALID.replace("language: rust", "language: go")),
        (
            "missing tests",
            VALID.replace("        tests:\n          projects:\n            - crates/math/Cargo.toml\n", ""),
        ),
        ("both test forms", format!("{VALID}          command: cargo test\n")),
        ("empty components", "components: {}\n".to_string()),
    ];
    for (name, yaml) in invalid {
        let problems = schema_errors(&schema, &yaml);
        assert!(!problems.is_empty(), "schema accepted {name}:\n{yaml}");
        assert!(parse_binding(&path, &yaml).is_err(), "loader accepted {name}:\n{yaml}");
    }

    let marked = format!("```yaml\n{MARKER}../../{SCHEMA_FILE}\n{VALID}```\n");
    assert_eq!(markdown_problems(&root, &schema, &path, &marked), Vec::<String>::new());

    let stale = marked.replace("language: rust", "language: go");
    assert!(!markdown_problems(&root, &schema, &path, &stale).is_empty());

    let wrong_schema_path = marked.replace("../../", "../");
    assert!(!markdown_problems(&root, &schema, &path, &wrong_schema_path).is_empty());

    let unmarked = format!("```yaml\n{VALID}```\n");
    assert!(!markdown_problems(&root, &schema, &path, &unmarked).is_empty());
    let exempt = root.join("docs/decisions/example.md");
    assert_eq!(markdown_problems(&root, &schema, &exempt, &unmarked), Vec::<String>::new());
}
