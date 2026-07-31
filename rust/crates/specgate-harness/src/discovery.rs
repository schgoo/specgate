//! Structural discovery — a component's self-described schema, normalized to
//! spec types and folded to the black-box spec view.
//!
//! [`discover`] is the structural complement to [`crate::run_spec`]'s
//! behavioral (trace) conformance: it reads a spec's component and EVERY bound
//! target, asks each self-describing target for its schema (via the runtime's
//! link-time `discovery_json()`), normalizes each to spec types, and folds
//! setups into their operation's inputs (the invisible-setup model). The
//! canonical `schema` is what the self-describing targets agree on; `targets`
//! enumerates every bound target and its outcome — a self-describing target
//! echoes its own full normalized schema, while a target with no discovery
//! mechanism is [`TargetOutcome::NotSelfDescribing`]. Because every bound
//! target appears in `targets`, divergence is implicit (a self-describing
//! target whose echoed schema differs from the canonical one is visible by
//! comparison) and a skipped target is visible rather than silently absent.
//!
//! This module also owns the shared discovery primitives — the temp-crate
//! discovery build, the registry parse, the invisible-setup folding, and the
//! type normalization — that `specgate extract` reuses to derive spec
//! skeletons. They live here (not in the CLI) so the harness and the extractor
//! share a single implementation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Public discover API
// ---------------------------------------------------------------------------

/// One black-box operation input, with any setup construction params folded in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredInput {
    pub name: String,
    /// Normalized spec type, e.g. `"i32"`, `"List<i32>"`, `"Option<i32>"`.
    pub ty: String,
}

/// One operation on the component's normalized surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredOperation {
    pub name: String,
    pub is_async: bool,
    pub inputs: Vec<DiscoveredInput>,
    /// Normalized spec return type; empty when the operation returns nothing.
    pub output: String,
}

/// One named field (struct field or enum-variant field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredField {
    pub name: String,
    pub ty: String,
}

/// One enum variant. `fields` is empty for unit/tuple variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredVariant {
    pub name: String,
    pub fields: Vec<DiscoveredField>,
}

/// One named complex type owned by the component. `kind` is `"struct"` or
/// `"enum"`; structs populate `fields`, enums populate `variants`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredType {
    pub name: String,
    pub kind: String,
    pub fields: Vec<DiscoveredField>,
    pub variants: Vec<DiscoveredVariant>,
}

/// A component's normalized, folded schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSchema {
    pub component: String,
    pub operations: Vec<DiscoveredOperation>,
    pub types: Vec<DiscoveredType>,
}

/// The self-description outcome for one bound target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetOutcome {
    /// The target self-describes, echoing its own full normalized schema.
    SelfDescribed { schema: DiscoveredSchema },
    /// The target has no discovery mechanism, so it emits no schema.
    NotSelfDescribing { reason: String },
}

/// One bound target (a binding file × its selected target) and its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDiscovery {
    /// Binding target label, `<binding-stem>::<target>`.
    pub target: String,
    pub outcome: TargetOutcome,
}

/// Outcome of [`discover`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverOutcome {
    Complete {
        schema: DiscoveredSchema,
        targets: Vec<TargetDiscovery>,
    },
    Error {
        reason: String,
    },
}

/// Report the component's self-described schema for the spec at `spec_path`.
///
/// Reads the spec's component and EVERY bound target, asks each self-describing
/// (Rust) target for its schema, normalizes each to spec types, folds setups
/// into operation inputs, and returns the canonical schema plus one
/// [`TargetDiscovery`] per bound target (in binding-list order). Rust targets
/// self-describe ([`TargetOutcome::SelfDescribed`]); languages without a
/// discovery mechanism (C#, etc.) are [`TargetOutcome::NotSelfDescribing`]. The
/// canonical schema is the first self-describing target's schema.
///
/// Never panics: any failure is returned as [`DiscoverOutcome::Error`].
#[must_use]
pub fn discover(spec_path: &str) -> DiscoverOutcome {
    match discover_inner(spec_path) {
        Ok(o) => o,
        Err(reason) => DiscoverOutcome::Error { reason },
    }
}

fn discover_inner(spec_path: &str) -> Result<DiscoverOutcome, String> {
    let path = PathBuf::from(spec_path);
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let raw = std::fs::read_to_string(&path).map_err(|_e| format!("spec file not found: {spec_path}"))?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|_e| "spec file is not valid YAML".to_string())?;
    let parsed = crate::spec::parse_spec(&yaml).map_err(|_e| "spec file is not valid YAML".to_string())?;
    let component = parsed.name.clone();

    if parsed.binding_paths.is_empty() {
        return Ok(DiscoverOutcome::Error {
            reason: "spec has no binding".to_string(),
        });
    }

    let mut canonical: Option<DiscoveredSchema> = None;
    let mut targets: Vec<TargetDiscovery> = Vec::new();

    for bp in &parsed.binding_paths {
        let binding_full = crate::spec::binding_path_resolved(&path, bp);
        let Some(binding) = crate::binding::load_binding(&binding_full) else {
            return Ok(DiscoverOutcome::Error {
                reason: format!("binding '{bp}' not found"),
            });
        };

        let label = binding_target_label(bp, parsed.target.as_deref());
        let outcome = discover_target(&binding, parsed.target.as_deref(), &component)?;

        if let TargetOutcome::SelfDescribed { schema } = &outcome
            && canonical.is_none()
        {
            canonical = Some(schema.clone());
        }

        targets.push(TargetDiscovery { target: label, outcome });
    }

    match canonical {
        Some(schema) => Ok(DiscoverOutcome::Complete { schema, targets }),
        None => Ok(DiscoverOutcome::Error {
            reason: format!("no self-describing target for component '{component}'"),
        }),
    }
}

/// Discover one bound target, dispatching on the binding's language. Rust
/// targets self-describe via link-time `discovery_json()`; C# targets
/// self-describe via reflection over the compiled fixture assembly (see
/// [`crate::csharp_discovery`]). Both normalize to the same spec types and fold
/// setups identically, so a conforming target echoes the canonical schema.
/// Languages without a discovery mechanism (a `command` target, etc.) report
/// [`TargetOutcome::NotSelfDescribing`]. Adding a new language's discovery is a
/// matter of extending this dispatch.
fn discover_target(binding: &crate::binding::Binding, target_name: Option<&str>, component: &str) -> Result<TargetOutcome, String> {
    match binding.language.as_str() {
        "rust" => {
            let Some(target) = binding.target(target_name) else {
                return Err(format!("target '{}' not found in binding", target_name.unwrap_or("<default>")));
            };
            let json = run_discovery(&target.package_root)?;
            let registry = Registry::parse(&json)?;
            let schema = build_schema(&registry, component);
            Ok(TargetOutcome::SelfDescribed { schema })
        }
        "csharp" => {
            let Some(target) = binding.target(target_name) else {
                return Err(format!("target '{}' not found in binding", target_name.unwrap_or("<default>")));
            };
            let json = crate::csharp_discovery::run_csharp_discovery(target, component)?;
            let registry = Registry::parse(&json)?;
            let schema = build_schema_prenormalized(&registry, component);
            Ok(TargetOutcome::SelfDescribed { schema })
        }
        other => Ok(TargetOutcome::NotSelfDescribing {
            reason: format!("no discovery metadata emitted by {other} target"),
        }),
    }
}

/// Build the canonical `DiscoveredSchema` for `comp` from a parsed registry:
/// operations (folded inputs, normalized output) sorted by name, plus the
/// component's own named types (fields/variants normalized) sorted by name.
fn build_schema(registry: &Registry, comp: &str) -> DiscoveredSchema {
    let type_names = registry.type_names();

    let operations = registry
        .operations_for(comp)
        .into_iter()
        .map(|op| DiscoveredOperation {
            name: op.name.clone(),
            is_async: op.is_async,
            inputs: build_inputs(op, registry)
                .into_iter()
                .map(|(name, ty)| DiscoveredInput { name, ty: ty.ref_string() })
                .collect(),
            output: if is_unit(&op.return_type) {
                String::new()
            } else {
                map_type(&op.return_type, &type_names).ref_string()
            },
        })
        .collect();

    let types = registry
        .local_types(comp)
        .into_iter()
        .map(|t| DiscoveredType {
            name: t.name.clone(),
            kind: t.kind.clone(),
            fields: t
                .fields
                .iter()
                .map(|(n, ty)| DiscoveredField {
                    name: n.clone(),
                    ty: map_type(ty, &type_names).ref_string(),
                })
                .collect(),
            variants: t
                .variants
                .iter()
                .map(|v| DiscoveredVariant {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|(n, ty)| DiscoveredField {
                            name: n.clone(),
                            ty: map_type(ty, &type_names).ref_string(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();

    DiscoveredSchema {
        component: comp.to_string(),
        operations,
        types,
    }
}

/// Build a `DiscoveredSchema` from a registry whose type strings are ALREADY
/// normalized spec-type references (as emitted by the C# discovery program).
///
/// The setup-folding is language-neutral, so this reuses [`raw_inputs`] to fold
/// setup construction params into each operation's inputs (the invisible-setup
/// model). It differs from [`build_schema`] only in that it does NOT re-run the
/// Rust type mapper over the (already-normalized) type strings — it passes them
/// through verbatim, so the resulting schema is byte-identical to the Rust
/// canonical when the C# target conforms.
fn build_schema_prenormalized(registry: &Registry, comp: &str) -> DiscoveredSchema {
    let operations = registry
        .operations_for(comp)
        .into_iter()
        .map(|op| DiscoveredOperation {
            name: op.name.clone(),
            is_async: op.is_async,
            inputs: raw_inputs(op, registry)
                .into_iter()
                .map(|(name, ty)| DiscoveredInput { name, ty })
                .collect(),
            output: if is_unit(&op.return_type) {
                String::new()
            } else {
                op.return_type.clone()
            },
        })
        .collect();

    let types = registry
        .local_types(comp)
        .into_iter()
        .map(|t| DiscoveredType {
            name: t.name.clone(),
            kind: t.kind.clone(),
            fields: t
                .fields
                .iter()
                .map(|(n, ty)| DiscoveredField {
                    name: n.clone(),
                    ty: ty.clone(),
                })
                .collect(),
            variants: t
                .variants
                .iter()
                .map(|v| DiscoveredVariant {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|(n, ty)| DiscoveredField {
                            name: n.clone(),
                            ty: ty.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();

    DiscoveredSchema {
        component: comp.to_string(),
        operations,
        types,
    }
}
/// used to identify each bound target in `targets`.
fn binding_target_label(binding_path: &str, target: Option<&str>) -> String {
    let stem = Path::new(binding_path).file_stem().and_then(|s| s.to_str()).unwrap_or(binding_path);
    format!("{stem}::{}", target.unwrap_or("default"))
}

// ---------------------------------------------------------------------------
// Discovery build
// ---------------------------------------------------------------------------

/// The Rust workspace root (`rust/`) of this repository, resolved at compile
/// time from this crate's manifest directory.
#[must_use]
pub fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/crates/specgate-harness
    p.pop(); // crates
    p.pop(); // rust
    p
}

/// Scaffold a temporary bin crate that links the target crate and prints its
/// `discovery_json()`, build+run it, and return the captured JSON.
///
/// # Errors
///
/// Returns an error string if the scaffold, build, or run fails, or if the
/// crate name cannot be read from `Cargo.toml`.
pub fn run_discovery(package_root: &Path) -> Result<String, String> {
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

#[must_use]
pub fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Convert a path to a forward-slash string suitable for `Cargo.toml`, stripping
/// the Windows extended-length prefix (`\\?\`) that `canonicalize` adds.
#[must_use]
pub fn to_cargo_path(p: &Path) -> String {
    let s = p.display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// Read the `[package] name` from a crate's `Cargo.toml`.
#[must_use]
pub fn cargo_package_name(package_root: &Path) -> Option<String> {
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
pub struct OpInfo {
    pub name: String,
    pub is_setup: bool,
    pub is_async: bool,
    pub return_type: String,
    pub fills: String,
    pub params: Vec<(String, String)>,
    pub component: String,
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub name: String,
    pub kind: String,
    pub fields: Vec<(String, String)>,
    pub variants: Vec<VariantInfo>,
    pub component: String,
}

#[derive(Debug, Clone)]
pub struct Registry {
    pub ops: Vec<OpInfo>,
    pub types: Vec<TypeInfo>,
}

impl Registry {
    /// Parse the runtime `discovery_json()` output into a [`Registry`].
    ///
    /// # Errors
    ///
    /// Returns an error string if the JSON is malformed or missing the
    /// `operations`/`types` arrays.
    pub fn parse(json: &str) -> Result<Self, String> {
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

    /// Non-setup operations owned by `comp`, sorted by name.
    #[must_use]
    pub fn operations_for(&self, comp: &str) -> Vec<&OpInfo> {
        let mut ops: Vec<&OpInfo> = self.ops.iter().filter(|o| !o.is_setup && o.component == comp).collect();
        ops.sort_by(|a, b| a.name.cmp(&b.name));
        ops
    }

    /// Registered types owned by `comp`, sorted by name.
    #[must_use]
    pub fn local_types(&self, comp: &str) -> Vec<&TypeInfo> {
        let mut ts: Vec<&TypeInfo> = self.types.iter().filter(|t| t.component == comp).collect();
        ts.sort_by(|a, b| a.name.cmp(&b.name));
        ts
    }

    /// Distinct, sorted components present among non-setup operations and types.
    #[must_use]
    pub fn present_components(&self) -> Vec<String> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        for o in &self.ops {
            if !o.is_setup && !o.component.is_empty() {
                set.insert(o.component.clone());
            }
        }
        for t in &self.types {
            if !t.component.is_empty() {
                set.insert(t.component.clone());
            }
        }
        set.into_iter().collect()
    }

    /// Setups registered for the operation named `op`, in registry order.
    #[must_use]
    pub fn setups_for<'a>(&'a self, op: &str) -> Vec<&'a OpInfo> {
        self.ops.iter().filter(|o| o.is_setup && o.name == op).collect()
    }

    /// The names of registered `SpecEvent` types.
    #[must_use]
    pub fn type_names(&self) -> Vec<&str> {
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
        component: str_field(v, "component"),
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
        component: str_field(v, "component"),
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
// Type-ref mapping
// ---------------------------------------------------------------------------

/// A mapped spec type reference: either a scalar shorthand string or an inline
/// `map`/`set` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecType {
    Scalar(String),
    Map { keys: Box<SpecType>, values: Box<SpecType> },
    Set { items: Box<SpecType> },
}

impl SpecType {
    /// Render as a single-line reference string (used inside shorthand wrappers
    /// such as `Option<…>` / `List<…>` / `Result<…>`).
    #[must_use]
    pub fn ref_string(&self) -> String {
        match self {
            SpecType::Scalar(s) => s.clone(),
            SpecType::Map { keys, values } => format!("map<{}, {}>", keys.ref_string(), values.ref_string()),
            SpecType::Set { items } => format!("set<{}>", items.ref_string()),
        }
    }
}

/// Parsed Rust type AST (only the shapes schema extraction needs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustType {
    Named { name: String, args: Vec<RustType> },
    Ref(Box<RustType>),
    Slice(Box<RustType>),
}

/// Map a stringified Rust type (as produced by `quote!(#ty).to_string()`, which
/// inserts spaces around tokens) to its spec type reference, recursing into
/// generic arguments. `type_names` are the registered `SpecEvent` types (passed
/// through by bare name).
#[must_use]
pub fn map_type(ty: &str, type_names: &[&str]) -> SpecType {
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
        // The runtime `Value` is the spec's built-in universal structured value.
        (_, 0) if is_runtime_value(name) => SpecType::Scalar("value".to_string()),
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

/// True for primitive scalars and the collection/option/result constructors
/// extraction maps directly — i.e. type names that are NOT named `SpecEvent` refs.
#[must_use]
pub fn is_builtin_type(name: &str) -> bool {
    is_runtime_value(name)
        || matches!(
            name,
            "String"
                | "str"
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
                | "Vec"
                | "Result"
                | "HashMap"
                | "BTreeMap"
                | "HashSet"
                | "BTreeSet"
        )
}

/// True for the runtime `specgate_runtime::Value` type in any of its stringified
/// forms. Type-path parsing keeps only the last path segment, so any of
/// `Value`, `specgate_runtime::Value`, `::specgate_runtime::Value`, or
/// `specgate::Value` arrive here as the bare name `Value`. It maps to the
/// built-in `value` spec type.
#[must_use]
pub fn is_runtime_value(name: &str) -> bool {
    matches!(
        name,
        "Value" | "specgate_runtime::Value" | "::specgate_runtime::Value" | "specgate::Value"
    )
}

/// Collect every non-builtin named type referenced inside a stringified Rust
/// type (recursing into generic args), in source order.
pub fn collect_named_refs(ty: &str, out: &mut Vec<String>) {
    if let Some(parsed) = parse_rust_type(ty) {
        collect_from_rust_type(&parsed, out);
    }
}

fn collect_from_rust_type(t: &RustType, out: &mut Vec<String>) {
    match t {
        RustType::Ref(inner) | RustType::Slice(inner) => collect_from_rust_type(inner, out),
        RustType::Named { name, args } => {
            if !is_builtin_type(name) {
                out.push(name.clone());
            }
            for a in args {
                collect_from_rust_type(a, out);
            }
        }
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
            // Skip a leading path separator (`::Foo` / `::specgate_runtime::Value`).
            if tok == "::" {
                *pos += 1;
            }
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
#[must_use]
pub fn normalize_type(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True for the unit return type (`()` or empty).
#[must_use]
pub fn is_unit(ty: &str) -> bool {
    let n = normalize_type(ty);
    n.is_empty() || n == "()"
}

// ---------------------------------------------------------------------------
// Inputs (setup-aware)
// ---------------------------------------------------------------------------

/// Build an operation's RAW (unmapped) `inputs` list, applying the
/// invisible-setup model: receiver-filling setups inject their construction
/// params at the front (in setup order); a signature param whose type a setup
/// fills is replaced by that setup's own construction params; remaining params
/// are kept in order. Values are the stringified Rust types (unmapped).
#[must_use]
pub fn raw_inputs(op: &OpInfo, registry: &Registry) -> Vec<(String, String)> {
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

    let mut out: Vec<(String, String)> = Vec::new();
    for s in &receiver_setups {
        for (pn, pty) in &s.params {
            out.push((pn.clone(), pty.clone()));
        }
    }
    for (pn, pty) in &op.params {
        if filled.contains(pn) {
            if let Some(inj) = param_injection.get(pn) {
                for (ipn, ipty) in inj {
                    out.push((ipn.clone(), ipty.clone()));
                }
            }
        } else {
            out.push((pn.clone(), pty.clone()));
        }
    }
    out
}

/// Build an operation's spec `inputs` list (mapped to `SpecType`).
#[must_use]
pub fn build_inputs(op: &OpInfo, registry: &Registry) -> Vec<(String, SpecType)> {
    let type_names = registry.type_names();
    raw_inputs(op, registry)
        .into_iter()
        .map(|(name, ty)| (name, map_type(&ty, &type_names)))
        .collect()
}
