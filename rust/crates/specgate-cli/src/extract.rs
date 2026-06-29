//! `specgate extract <package_root> -o <out>` — deterministically derive a
//! schema-only spec skeleton from an annotated Rust crate.
//!
//! Extraction reads the crate's *link-time* operation/type registry (the
//! `#[spec_operation]` / `#[spec_setup]` / `SpecEvent` metadata collected via
//! `linkme`) rather than parsing source or invoking an LLM. A tiny discovery
//! binary is scaffolded that depends on the target crate, calls
//! `specgate::__rt::discovery_json()`, and prints the registry as JSON; the
//! JSON is then mapped to a `.spec.yaml` skeleton plus a sibling binding file.
//!
//! This is "Part A": only the schema (operations, inputs/outputs, types) is
//! derived. Test `cases:` come from trace collection ("Part B") and are emitted
//! empty here, so a freshly-extracted skeleton validates as sound except for the
//! expected `no_cases` finding.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use specgate::{SpecEvent, spec_operation};

/// Summary of an extraction run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct ExtractReport {
    #[spec_event]
    pub spec_name: String,
    #[spec_event]
    pub operations: i32,
    #[spec_event]
    pub types: i32,
    #[spec_event]
    pub output_path: String,
}

/// Outcome of `extract`.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum ExtractOutcome {
    Complete { report: ExtractReport },
    Error { reason: String },
}

impl std::fmt::Display for ExtractOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractOutcome::Complete { report } => write!(
                f,
                "Complete(spec={}, operations={}, types={}, output={})",
                report.spec_name, report.operations, report.types, report.output_path
            ),
            ExtractOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Extract a schema-only spec skeleton from the annotated crate at
/// `package_root`, writing the `.spec.yaml` to `out` and a sibling binding file.
///
/// Returns [`ExtractOutcome::Error`] (never panics) when `package_root` does not
/// exist, is not a Rust crate (no `Cargo.toml`), the discovery build fails, or
/// the output cannot be written.
#[must_use]
#[spec_operation("extract")]
pub fn extract(package_root: &str, out: &str) -> ExtractOutcome {
    let root = Path::new(package_root);
    if !root.exists() {
        return ExtractOutcome::Error {
            reason: format!("package_root not found: {package_root}"),
        };
    }
    if !root.join("Cargo.toml").is_file() {
        return ExtractOutcome::Error {
            reason: format!("not a Rust crate (no Cargo.toml): {package_root}"),
        };
    }

    let json = match run_discovery(root) {
        Ok(j) => j,
        Err(reason) => return ExtractOutcome::Error { reason },
    };

    let registry = match Registry::parse(&json) {
        Ok(r) => r,
        Err(reason) => return ExtractOutcome::Error { reason },
    };

    let Some(crate_name) = cargo_package_name(root) else {
        return ExtractOutcome::Error {
            reason: format!("could not read [package] name from {package_root}/Cargo.toml"),
        };
    };
    let spec_name = derive_spec_name(&crate_name);

    let out_path = Path::new(out);
    let binding_file_name = binding_file_name(out_path);
    let spec_yaml = render_spec(&spec_name, &binding_file_name, &registry);

    // Create the output directory first so the binding's relative package_root
    // can be computed against a path that exists on disk (canonicalize requires
    // the directory to be present, otherwise it falls back to a verbatim path).
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return ExtractOutcome::Error {
            reason: format!("failed to create output directory {}: {e}", parent.display()),
        };
    }

    // The binding's package_root is relative to the binding file (= out's dir).
    let out_dir = out_path.parent().unwrap_or_else(|| Path::new("."));
    let rel_pkg = relative_path(out_dir, root);
    let binding_yaml = render_binding(&rel_pkg);

    if let Err(e) = std::fs::write(out_path, &spec_yaml) {
        return ExtractOutcome::Error {
            reason: format!("failed to write spec to {out}: {e}"),
        };
    }
    let binding_path = out_dir.join(&binding_file_name);
    if let Err(e) = std::fs::write(&binding_path, &binding_yaml) {
        return ExtractOutcome::Error {
            reason: format!("failed to write binding to {}: {e}", binding_path.display()),
        };
    }

    let report = ExtractReport {
        spec_name,
        operations: i32::try_from(registry.operations().count()).unwrap_or(i32::MAX),
        types: i32::try_from(registry.types.len()).unwrap_or(i32::MAX),
        output_path: out.to_string(),
    };
    ExtractOutcome::Complete { report }
}

// ---------------------------------------------------------------------------
// Discovery build
// ---------------------------------------------------------------------------

/// The Rust workspace root (`rust/`) of this repository, resolved at compile
/// time from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/crates/specgate-cli
    p.pop(); // crates
    p.pop(); // rust
    p
}

/// Scaffold a temporary bin crate that links the target crate and prints its
/// `discovery_json()`, build+run it, and return the captured JSON.
fn run_discovery(package_root: &Path) -> Result<String, String> {
    let ws = workspace_root();
    let specgate_path = ws.join("crates").join("specgate");
    let pkg_abs = std::fs::canonicalize(package_root).map_err(|e| format!("cannot resolve package_root: {e}"))?;
    let crate_name = cargo_package_name(package_root).ok_or_else(|| "could not read crate name from Cargo.toml".to_string())?;
    let rust_ident = crate_name.replace('-', "_");

    let scratch = ws.join("target").join("specgate-extract").join(&crate_name);
    std::fs::create_dir_all(scratch.join("src")).map_err(|e| format!("failed to scaffold discovery crate: {e}"))?;

    let manifest = format!(
        "[package]\nname = \"sg-extract-discovery\"\nversion = \"0.0.1\"\nedition = \"2024\"\n\n[[bin]]\nname = \"sg-extract-discovery\"\npath = \"src/main.rs\"\n\n[dependencies]\nspecgate = {{ path = \"{specgate}\" }}\n{crate_name} = {{ path = \"{pkg}\" }}\n\n[workspace]\n",
        specgate = to_cargo_path(&specgate_path),
        pkg = to_cargo_path(&pkg_abs),
    );
    std::fs::write(scratch.join("Cargo.toml"), manifest).map_err(|e| format!("failed to write discovery manifest: {e}"))?;

    // Seed the discovery crate's lockfile from the workspace so cargo need not
    // reach crates.io (the environment may have it blocked).
    let parent_lock = ws.join("Cargo.lock");
    if parent_lock.exists() {
        let _ = std::fs::copy(&parent_lock, scratch.join("Cargo.lock"));
    }

    // `extern crate` forces the target crate's rlib to be linked so its
    // `linkme` registration statics (which are `#[used]`) are pulled in.
    let main_rs = format!("extern crate {rust_ident};\nfn main() {{\n    print!(\"{{}}\", specgate::__rt::discovery_json());\n}}\n");
    std::fs::write(scratch.join("src").join("main.rs"), main_rs).map_err(|e| format!("failed to write discovery main.rs: {e}"))?;

    let mut cmd = Command::new(cargo_bin());
    cmd.arg("run").arg("--quiet").arg("--manifest-path").arg(scratch.join("Cargo.toml"));
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    cmd.env_remove("CARGO");
    cmd.env_remove("CARGO_MANIFEST_DIR");
    cmd.env("CARGO_TARGET_DIR", scratch.join("target").as_os_str());

    let output = cmd.output().map_err(|e| format!("failed to run discovery build: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "discovery build failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let json = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if json.is_empty() {
        return Err("discovery build produced no output".to_string());
    }
    Ok(json)
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Convert a path to a forward-slash string suitable for `Cargo.toml`, stripping
/// the Windows extended-length prefix (`\\?\`) that `canonicalize` adds.
fn to_cargo_path(p: &Path) -> String {
    let s = p.display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// Read the `[package] name` from a crate's `Cargo.toml`.
fn cargo_package_name(package_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(package_root.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_package = false;
        }
        if in_package && let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start_matches([' ', '\t', '=']).trim();
            let name = rest.trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Registry model (parsed from discovery JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct OpInfo {
    name: String,
    is_setup: bool,
    is_async: bool,
    return_type: String,
    fills: String,
    params: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct VariantInfo {
    name: String,
    fields: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct TypeInfo {
    name: String,
    kind: String,
    fields: Vec<(String, String)>,
    variants: Vec<VariantInfo>,
}

#[derive(Debug, Clone)]
struct Registry {
    ops: Vec<OpInfo>,
    types: Vec<TypeInfo>,
}

impl Registry {
    fn parse(json: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("failed to parse discovery JSON: {e}"))?;
        let ops = v
            .get("operations")
            .and_then(serde_json::Value::as_array)
            .ok_or("discovery JSON missing 'operations' array")?
            .iter()
            .map(parse_op)
            .collect();
        let mut types: Vec<TypeInfo> = v
            .get("types")
            .and_then(serde_json::Value::as_array)
            .ok_or("discovery JSON missing 'types' array")?
            .iter()
            .map(parse_type)
            .collect();
        types.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Registry { ops, types })
    }

    /// All non-setup operations, sorted by name. Setups are invisible.
    fn operations(&self) -> impl Iterator<Item = &OpInfo> {
        let mut ops: Vec<&OpInfo> = self.ops.iter().filter(|o| !o.is_setup).collect();
        ops.sort_by(|a, b| a.name.cmp(&b.name));
        ops.into_iter()
    }

    /// Setups registered for the operation named `op`, in registry order.
    fn setups_for<'a>(&'a self, op: &str) -> Vec<&'a OpInfo> {
        self.ops.iter().filter(|o| o.is_setup && o.name == op).collect()
    }

    /// The names of registered `SpecEvent` types.
    fn type_names(&self) -> Vec<&str> {
        self.types.iter().map(|t| t.name.as_str()).collect()
    }
}

fn parse_op(v: &serde_json::Value) -> OpInfo {
    OpInfo {
        name: str_field(v, "name"),
        is_setup: v.get("is_setup").and_then(serde_json::Value::as_bool).unwrap_or(false),
        is_async: v.get("is_async").and_then(serde_json::Value::as_bool).unwrap_or(false),
        return_type: str_field(v, "return_type"),
        fills: str_field(v, "fills"),
        params: parse_pairs(v.get("params")),
    }
}

fn parse_type(v: &serde_json::Value) -> TypeInfo {
    let variants = v
        .get("variants")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|vv| VariantInfo {
                    name: str_field(vv, "name"),
                    fields: parse_pairs(vv.get("fields")),
                })
                .collect()
        })
        .unwrap_or_default();
    TypeInfo {
        name: str_field(v, "name"),
        kind: str_field(v, "kind"),
        fields: parse_pairs(v.get("fields")),
        variants,
    }
}

fn parse_pairs(v: Option<&serde_json::Value>) -> Vec<(String, String)> {
    v.and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let pair = p.as_array()?;
                    Some((pair.first()?.as_str()?.to_string(), pair.get(1)?.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
}

// ---------------------------------------------------------------------------
// Spec-name derivation
// ---------------------------------------------------------------------------

/// Derive the spec `name` from a crate's cargo name: strip a leading
/// `specgate-` / `specgate_` prefix and replace remaining `-` with `_`. E.g.
/// `specgate-extract-fixture` → `extract_fixture`.
fn derive_spec_name(crate_name: &str) -> String {
    let stripped = crate_name
        .strip_prefix("specgate-")
        .or_else(|| crate_name.strip_prefix("specgate_"))
        .unwrap_or(crate_name);
    stripped.replace('-', "_")
}

/// Binding file name beside the spec: `<stem>.binding.yaml`, where `<stem>` is
/// the spec file name with a trailing `.spec.yaml` / `.yaml` removed.
fn binding_file_name(out: &Path) -> String {
    let file = out.file_name().and_then(|n| n.to_str()).unwrap_or("extracted.spec.yaml");
    let stem = file
        .strip_suffix(".spec.yaml")
        .or_else(|| file.strip_suffix(".yaml"))
        .unwrap_or(file);
    format!("{stem}.binding.yaml")
}

/// Compute a relative path from `from_dir` to `to` using canonicalized absolute
/// paths, falling back to the original `to` if either cannot be resolved.
fn relative_path(from_dir: &Path, to: &Path) -> String {
    let from = std::fs::canonicalize(from_dir).ok();
    let to_abs = std::fs::canonicalize(to).ok();
    let (Some(from), Some(to_abs)) = (from, to_abs) else {
        return to_cargo_path(to);
    };
    let from_comps: Vec<_> = from.components().collect();
    let to_comps: Vec<_> = to_abs.components().collect();
    let common = from_comps.iter().zip(to_comps.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_comps.len() {
        parts.push("..".to_string());
    }
    for c in &to_comps[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

// ---------------------------------------------------------------------------
// Type-ref mapping
// ---------------------------------------------------------------------------

/// A mapped spec type reference: either a scalar shorthand string or an inline
/// `map`/`set` object.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpecType {
    Scalar(String),
    Map { keys: Box<SpecType>, values: Box<SpecType> },
    Set { items: Box<SpecType> },
}

impl SpecType {
    /// Render as a single-line reference string (used inside shorthand wrappers
    /// such as `Option<…>` / `List<…>` / `Result<…>`).
    fn ref_string(&self) -> String {
        match self {
            SpecType::Scalar(s) => s.clone(),
            SpecType::Map { keys, values } => format!("map<{}, {}>", keys.ref_string(), values.ref_string()),
            SpecType::Set { items } => format!("set<{}>", items.ref_string()),
        }
    }
}

/// Parsed Rust type AST (only the shapes Part-A extraction needs).
#[derive(Debug, Clone, PartialEq, Eq)]
enum RustType {
    Named { name: String, args: Vec<RustType> },
    Ref(Box<RustType>),
    Slice(Box<RustType>),
}

/// Map a stringified Rust type (as produced by `quote!(#ty).to_string()`, which
/// inserts spaces around tokens) to its spec type reference, recursing into
/// generic arguments. `type_names` are the registered `SpecEvent` types (passed
/// through by bare name).
fn map_type(ty: &str, type_names: &[&str]) -> SpecType {
    let parsed = parse_rust_type(ty).unwrap_or_else(|| RustType::Named {
        name: ty.trim().to_string(),
        args: Vec::new(),
    });
    map_rust_type(&parsed, type_names)
}

fn map_rust_type(t: &RustType, type_names: &[&str]) -> SpecType {
    match t {
        RustType::Ref(inner) => map_rust_type(inner, type_names),
        RustType::Slice(inner) => SpecType::Scalar(format!("List<{}>", map_rust_type(inner, type_names).ref_string())),
        RustType::Named { name, args } => map_named(name, args, type_names),
    }
}

fn map_named(name: &str, args: &[RustType], type_names: &[&str]) -> SpecType {
    let m = |t: &RustType| map_rust_type(t, type_names);
    match (name, args.len()) {
        ("String" | "str", _) => SpecType::Scalar("string".to_string()),
        ("i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "f32" | "f64" | "bool", _) => {
            SpecType::Scalar(name.to_string())
        }
        ("Option", 1) => SpecType::Scalar(format!("Option<{}>", m(&args[0]).ref_string())),
        ("Vec", 1) => SpecType::Scalar(format!("List<{}>", m(&args[0]).ref_string())),
        ("Result", 2) => SpecType::Scalar(format!("Result<{}, {}>", m(&args[0]).ref_string(), m(&args[1]).ref_string())),
        ("HashMap" | "BTreeMap", 2) => SpecType::Map {
            keys: Box::new(m(&args[0])),
            values: Box::new(m(&args[1])),
        },
        ("HashSet" | "BTreeSet", 1) => SpecType::Set {
            items: Box::new(m(&args[0])),
        },
        // Named SpecEvent type or any other bare name → pass through by name.
        _ => SpecType::Scalar(name.to_string()),
    }
}

/// Parse a (possibly space-separated) Rust type string into a [`RustType`].
fn parse_rust_type(s: &str) -> Option<RustType> {
    let tokens = tokenize_type(s);
    let mut pos = 0;
    let t = parse_type_tokens(&tokens, &mut pos)?;
    Some(t)
}

fn tokenize_type(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
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

fn parse_type_tokens(tokens: &[String], pos: &mut usize) -> Option<RustType> {
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
            let inner = parse_type_tokens(tokens, pos)?;
            Some(RustType::Ref(Box::new(inner)))
        }
        "[" => {
            *pos += 1;
            let inner = parse_type_tokens(tokens, pos)?;
            // Expect closing ']'.
            if tokens.get(*pos).map(String::as_str) == Some("]") {
                *pos += 1;
            }
            Some(RustType::Slice(Box::new(inner)))
        }
        _ => {
            // A path like `std :: collections :: BTreeMap` — keep the last segment.
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
                    if tokens.get(*pos).map(String::as_str) == Some(">") {
                        *pos += 1;
                        break;
                    }
                    let arg = parse_type_tokens(tokens, pos)?;
                    args.push(arg);
                    match tokens.get(*pos).map(String::as_str) {
                        Some(",") => {
                            *pos += 1;
                        }
                        Some(">") => {
                            *pos += 1;
                            break;
                        }
                        _ => break,
                    }
                }
            }
            Some(RustType::Named { name, args })
        }
    }
}

/// Normalize a type string for equality comparison (collapse whitespace).
fn normalize_type(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Inputs (setup-aware)
// ---------------------------------------------------------------------------

/// Build an operation's spec `inputs` list, applying the invisible-setup model:
/// receiver-filling setups inject their construction params at the front (in
/// setup order); a signature param whose type a setup fills is replaced by that
/// setup's own construction params; remaining params are kept in order.
fn build_inputs(op: &OpInfo, registry: &Registry) -> Vec<(String, SpecType)> {
    let type_names = registry.type_names();
    let setups = registry.setups_for(&op.name);

    // For each signature param, the construction params injected in its place
    // when a setup fills it (empty vec ⇒ omit the param with nothing injected).
    let mut param_injection: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut filled: Vec<String> = Vec::new();
    let mut receiver_setups: Vec<&OpInfo> = Vec::new();

    for s in setups {
        let target = if s.fills.is_empty() {
            op.params
                .iter()
                .find(|(pn, pty)| normalize_type(pty) == normalize_type(&s.return_type) && !filled.contains(pn))
                .map(|(pn, _)| pn.clone())
        } else {
            op.params.iter().find(|(pn, _)| *pn == s.fills).map(|(pn, _)| pn.clone())
        };
        match target {
            Some(pn) => {
                filled.push(pn.clone());
                param_injection.insert(pn, s.params.clone());
            }
            None => receiver_setups.push(s),
        }
    }

    let mut out: Vec<(String, SpecType)> = Vec::new();
    for s in &receiver_setups {
        for (pn, pty) in &s.params {
            out.push((pn.clone(), map_type(pty, &type_names)));
        }
    }
    for (pn, pty) in &op.params {
        if filled.contains(pn) {
            if let Some(inj) = param_injection.get(pn) {
                for (ipn, ipty) in inj {
                    out.push((ipn.clone(), map_type(ipty, &type_names)));
                }
            }
        } else {
            out.push((pn.clone(), map_type(pty, &type_names)));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// YAML emission
// ---------------------------------------------------------------------------

const SPEC_HEADER: &str = "# Schema-only spec skeleton generated by `specgate extract` (Part A).\n\
#\n\
# Derived deterministically from the crate's link-time #[spec_operation] /\n\
# SpecEvent registry (no source parsing, no LLM). Determinism rules:\n\
#   * operations and types are sorted by name;\n\
#   * within an operation, inputs list receiver-setup construction params first\n\
#     (in setup order), then the entry point's own params in signature order,\n\
#     omitting any param a setup fills;\n\
#   * struct field order and enum variant order are preserved from source;\n\
#   * type refs: scalars (i32/i64/f64/bool/string); Option<T> / List<T> /\n\
#     Result<T, E> as string shorthand; map/set as inline objects; named\n\
#     SpecEvent types by name.\n\
#\n\
# `cases:` is intentionally empty — Part A extracts only the schema from\n\
# annotations; test cases come from trace collection (Part B). Regenerate with\n\
# `specgate extract <package_root> -o <out>`; do not edit by hand.\n";

const BINDING_HEADER: &str = "# Binding generated by `specgate extract` alongside the spec skeleton. The\n\
# single `default` target points at the extracted crate; the harness locates\n\
# each operation by its #[spec_operation] name. Paths are relative to this file.\n";

/// Quote a scalar reference string when it contains generic-bracket syntax that
/// would otherwise be ambiguous as an emitted spec type (`Option<T>`, etc.).
fn quote_ref(s: &str) -> String {
    if s.contains('<') { format!("\"{s}\"") } else { s.to_string() }
}

/// Render the full spec skeleton YAML.
fn render_spec(spec_name: &str, binding_file: &str, registry: &Registry) -> String {
    let type_names = registry.type_names();
    let mut s = String::new();
    s.push_str(SPEC_HEADER);
    s.push_str("spec_version: \"0.4.0\"\n");
    let _ = writeln!(s, "name: {spec_name}");
    let _ = writeln!(s, "binding: {binding_file}");

    if !registry.types.is_empty() {
        s.push_str("\ntypes:\n");
        for t in &registry.types {
            render_type(&mut s, t, &type_names);
        }
    }

    s.push_str("\noperations:\n");
    for op in registry.operations() {
        render_operation(&mut s, op, registry, &type_names);
    }

    s.push_str("\ncases: []\n");
    s
}

fn render_type(s: &mut String, t: &TypeInfo, type_names: &[&str]) {
    let _ = writeln!(s, "  {}:", t.name);
    if t.kind == "enum" {
        s.push_str("    oneof:\n");
        for v in &t.variants {
            if v.fields.is_empty() {
                let _ = writeln!(s, "      {}: {{}}", v.name);
            } else {
                let _ = writeln!(s, "      {}:", v.name);
                for (fname, fty) in &v.fields {
                    let _ = writeln!(s, "        {}: {}", fname, quote_ref(&map_type(fty, type_names).ref_string()));
                }
            }
        }
    } else {
        for (fname, fty) in &t.fields {
            let _ = writeln!(s, "    {}: {}", fname, quote_ref(&map_type(fty, type_names).ref_string()));
        }
    }
}

fn render_operation(s: &mut String, op: &OpInfo, registry: &Registry, type_names: &[&str]) {
    let _ = writeln!(s, "  {}:", op.name);
    if op.is_async {
        s.push_str("    async: true\n");
    }
    let inputs = build_inputs(op, registry);
    if !inputs.is_empty() {
        s.push_str("    inputs:\n");
        for (name, ty) in &inputs {
            // Inputs are emitted plain (a `key: value` line); scalar refs such
            // as `List<i32>` are valid unquoted plain YAML scalars.
            match ty {
                SpecType::Scalar(v) => {
                    let _ = writeln!(s, "      {name}: {v}");
                }
                SpecType::Map { keys, values } => {
                    let _ = writeln!(s, "      {name}:");
                    let _ = writeln!(s, "        type: map");
                    let _ = writeln!(s, "        keys: {}", quote_ref(&keys.ref_string()));
                    let _ = writeln!(s, "        values: {}", quote_ref(&values.ref_string()));
                }
                SpecType::Set { items } => {
                    let _ = writeln!(s, "      {name}:");
                    let _ = writeln!(s, "        type: set");
                    let _ = writeln!(s, "        items: {}", quote_ref(&items.ref_string()));
                }
            }
        }
    }
    // Unit return ⇒ no `$result` output.
    let ret = map_type(&op.return_type, type_names);
    if !is_unit(&op.return_type) {
        s.push_str("    outputs:\n");
        match &ret {
            SpecType::Scalar(v) => {
                let _ = writeln!(s, "      - $result: {}", quote_ref(v));
            }
            SpecType::Map { keys, values } => {
                s.push_str("      - $result:\n");
                s.push_str("          type: map\n");
                let _ = writeln!(s, "          keys: {}", quote_ref(&keys.ref_string()));
                let _ = writeln!(s, "          values: {}", quote_ref(&values.ref_string()));
            }
            SpecType::Set { items } => {
                s.push_str("      - $result:\n");
                s.push_str("          type: set\n");
                let _ = writeln!(s, "          items: {}", quote_ref(&items.ref_string()));
            }
        }
    }
}

fn is_unit(ty: &str) -> bool {
    let n = normalize_type(ty);
    n.is_empty() || n == "()"
}

fn render_binding(rel_pkg: &str) -> String {
    let mut s = String::new();
    s.push_str(BINDING_HEADER);
    s.push_str("language: rust\n");
    s.push_str("targets:\n");
    s.push_str("  default:\n");
    let _ = writeln!(s, "    package_root: {rel_pkg}");
    s
}

// ---------------------------------------------------------------------------
// Terminal formatting
// ---------------------------------------------------------------------------

/// Render an extract outcome to a colored, human-readable string for the
/// terminal (used by the binary).
#[must_use]
pub fn format_outcome(outcome: &ExtractOutcome) -> String {
    let mut s = String::new();
    match outcome {
        ExtractOutcome::Error { reason } => {
            let _ = writeln!(s, "\x1b[31merror:\x1b[0m {reason}");
        }
        ExtractOutcome::Complete { report } => {
            let _ = writeln!(s, "spec: {}", report.spec_name);
            let _ = writeln!(
                s,
                "\x1b[32mextracted:\x1b[0m {} operation(s), {} type(s) -> {}",
                report.operations, report.types, report.output_path
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<&'static str> {
        vec!["Shape", "Balance", "Money"]
    }

    #[test]
    fn maps_scalars() {
        let n = names();
        assert_eq!(map_type("i32", &n), SpecType::Scalar("i32".into()));
        assert_eq!(map_type("i64", &n), SpecType::Scalar("i64".into()));
        assert_eq!(map_type("f64", &n), SpecType::Scalar("f64".into()));
        assert_eq!(map_type("bool", &n), SpecType::Scalar("bool".into()));
        assert_eq!(map_type("String", &n), SpecType::Scalar("string".into()));
        assert_eq!(map_type("& str", &n), SpecType::Scalar("string".into()));
    }

    #[test]
    fn maps_named_spec_event_type() {
        assert_eq!(map_type("Shape", &names()), SpecType::Scalar("Shape".into()));
        assert_eq!(map_type("Balance", &names()), SpecType::Scalar("Balance".into()));
    }

    #[test]
    fn maps_option_list_result_shorthands() {
        let n = names();
        assert_eq!(map_type("Option < i32 >", &n).ref_string(), "Option<i32>");
        assert_eq!(map_type("Vec < i32 >", &n).ref_string(), "List<i32>");
        assert_eq!(map_type("& [ i32 ]", &n).ref_string(), "List<i32>");
        assert_eq!(map_type("Result < i32, String >", &n).ref_string(), "Result<i32, string>");
    }

    #[test]
    fn maps_map_and_set_to_inline_objects() {
        let n = names();
        assert_eq!(
            map_type("BTreeMap < String, i32 >", &n),
            SpecType::Map {
                keys: Box::new(SpecType::Scalar("string".into())),
                values: Box::new(SpecType::Scalar("i32".into())),
            }
        );
        assert_eq!(
            map_type("BTreeSet < String >", &n),
            SpecType::Set {
                items: Box::new(SpecType::Scalar("string".into())),
            }
        );
        assert_eq!(
            map_type("std :: collections :: HashMap < String, i32 >", &n),
            SpecType::Map {
                keys: Box::new(SpecType::Scalar("string".into())),
                values: Box::new(SpecType::Scalar("i32".into())),
            }
        );
    }

    #[test]
    fn maps_nested_generics() {
        let n = names();
        assert_eq!(map_type("Vec < Option < i32 > >", &n).ref_string(), "List<Option<i32>>");
    }

    #[test]
    fn derives_spec_name() {
        assert_eq!(derive_spec_name("specgate-extract-fixture"), "extract_fixture");
        assert_eq!(derive_spec_name("specgate_extract_fixture"), "extract_fixture");
        assert_eq!(derive_spec_name("my-crate"), "my_crate");
        assert_eq!(derive_spec_name("plain"), "plain");
    }

    #[test]
    fn binding_name_from_out() {
        assert_eq!(binding_file_name(Path::new("a/b/extracted.spec.yaml")), "extracted.binding.yaml");
        assert_eq!(binding_file_name(Path::new("foo.yaml")), "foo.binding.yaml");
    }

    #[test]
    fn is_unit_detects_void() {
        assert!(is_unit("()"));
        assert!(is_unit(""));
        assert!(!is_unit("i32"));
    }

    #[test]
    fn quote_ref_quotes_generics_only() {
        assert_eq!(quote_ref("i32"), "i32");
        assert_eq!(quote_ref("Shape"), "Shape");
        assert_eq!(quote_ref("Option<i32>"), "\"Option<i32>\"");
        assert_eq!(quote_ref("Result<i32, string>"), "\"Result<i32, string>\"");
    }

    // --- setup-fill logic -------------------------------------------------

    fn op(name: &str, is_setup: bool, return_type: &str, fills: &str, params: &[(&str, &str)]) -> OpInfo {
        OpInfo {
            name: name.into(),
            is_setup,
            is_async: false,
            return_type: return_type.into(),
            fills: fills.into(),
            params: params.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect(),
        }
    }

    #[test]
    fn setup_fills_param_by_type_omits_input() {
        // double(x: i32) with setup seed() -> i32 fills x; seed has no params.
        let reg = Registry {
            ops: vec![op("double", false, "i32", "", &[("x", "i32")]), op("double", true, "i32", "", &[])],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        assert!(inputs.is_empty(), "x is setup-filled and omitted: {inputs:?}");
    }

    #[test]
    fn receiver_setup_injects_params_at_front() {
        // scale(value: i32) method; setup make_scaler(factor: i32) -> Scaler
        // fills the receiver, injecting `factor` before the kept `value`.
        let reg = Registry {
            ops: vec![
                op("scale", false, "i32", "", &[("value", "i32")]),
                op("scale", true, "Scaler", "", &[("factor", "i32")]),
            ],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["factor", "value"]);
    }

    #[test]
    fn no_setup_keeps_params_in_order() {
        let reg = Registry {
            ops: vec![op("add", false, "i32", "", &[("a", "i32"), ("b", "i32")])],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn fills_pins_specific_param() {
        // Two i32 params; a setup with fills="b" must target b, not a.
        let reg = Registry {
            ops: vec![
                op("f", false, "i32", "", &[("a", "i32"), ("b", "i32")]),
                op("f", true, "i32", "b", &[]),
            ],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a"], "only b is filled/omitted");
    }

    #[test]
    fn registry_parses_and_filters_setups() {
        let json = r#"{"operations":[
            {"name":"add","is_setup":false,"is_async":false,"return_type":"i32","fills":"","params":[["a","i32"]]},
            {"name":"double","is_setup":true,"is_async":false,"return_type":"i32","fills":"","params":[]}
        ],"types":[
            {"name":"Money","module_path":"m","kind":"struct","fields":[["cents","i64"]],"variants":[]}
        ]}"#;
        let reg = Registry::parse(json).expect("parse");
        assert_eq!(reg.operations().count(), 1);
        assert_eq!(reg.types.len(), 1);
        assert_eq!(reg.setups_for("double").len(), 1);
    }

    #[test]
    fn renders_struct_and_enum_types() {
        let reg = Registry {
            ops: vec![],
            types: vec![
                TypeInfo {
                    name: "Money".into(),
                    kind: "struct".into(),
                    fields: vec![("cents".into(), "i64".into())],
                    variants: vec![],
                },
                TypeInfo {
                    name: "Shape".into(),
                    kind: "enum".into(),
                    fields: vec![],
                    variants: vec![
                        VariantInfo {
                            name: "Circle".into(),
                            fields: vec![("radius".into(), "f64".into())],
                        },
                        VariantInfo {
                            name: "Point".into(),
                            fields: vec![],
                        },
                    ],
                },
            ],
        };
        let yaml = render_spec("demo", "demo.binding.yaml", &reg);
        assert!(yaml.contains("  Money:\n    cents: i64\n"), "{yaml}");
        assert!(
            yaml.contains("  Shape:\n    oneof:\n      Circle:\n        radius: f64\n      Point: {}\n"),
            "{yaml}"
        );
        assert!(yaml.ends_with("\ncases: []\n"));
    }

    #[test]
    fn renders_binding() {
        let b = render_binding("..");
        assert!(b.contains("language: rust\n"));
        assert!(b.contains("    package_root: ..\n"));
    }

    #[test]
    fn error_when_package_root_missing() {
        let out = extract("definitely/does/not/exist", "x.spec.yaml");
        match out {
            ExtractOutcome::Error { reason } => assert!(reason.contains("package_root not found"), "{reason}"),
            ExtractOutcome::Complete { report } => panic!("expected error, got {report:?}"),
        }
    }

    #[test]
    fn error_when_not_a_crate() {
        // `src` (this crate's source dir, the test CWD's child) exists but has
        // no Cargo.toml — extraction must reject it before writing anything.
        let out = extract("src", "x.spec.yaml");
        match out {
            ExtractOutcome::Error { reason } => assert!(reason.contains("no Cargo.toml"), "{reason}"),
            ExtractOutcome::Complete { report } => panic!("expected error, got {report:?}"),
        }
    }

    #[test]
    fn display_formats_outcomes() {
        let c = ExtractOutcome::Complete {
            report: ExtractReport {
                spec_name: "x".into(),
                operations: 2,
                types: 1,
                output_path: "o".into(),
            },
        };
        assert!(format!("{c}").contains("Complete(spec=x"));
        assert!(format_outcome(&c).contains("extracted:"));
        let e = ExtractOutcome::Error { reason: "boom".into() };
        assert_eq!(format!("{e}"), "Error(boom)");
        assert!(format_outcome(&e).contains("boom"));
    }
}
