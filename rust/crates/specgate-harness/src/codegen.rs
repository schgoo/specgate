//! Generate a temporary Cargo project that compiles + executes a fixture
//! against the spec's cases and writes a JSON trace to disk.

use crate::binding::Runtime;
use crate::scan::{AnnotatedSource, OpDecl};
use crate::spec::{Case, Spec};
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub struct GeneratedProject {
    pub crate_dir: PathBuf,
    pub trace_file: PathBuf,
}

/// Configuration for code generation.
pub struct GenerateConfig<'a> {
    pub spec: &'a Spec,
    pub cases_to_run: &'a [&'a Case],
    pub annotated: &'a AnnotatedSource,
    pub workspace_root: &'a Path,
    pub needs_async: bool,
    /// Async runtime the runner uses to drive async ops (only relevant when
    /// `needs_async`).
    pub runtime: Runtime,
    /// Resolved public linked path(s) for the target crate's contributing
    /// modules. Produced by [`resolve_fixture_crates`] before scaffolding.
    pub fixture_crates: &'a [FixtureCrateInfo],
    pub is_local: bool,
}

/// Information about the fixture crate for use as a Cargo dependency.
pub(crate) struct FixtureCrateInfo {
    /// The `name` field from the fixture crate's Cargo.toml (e.g., `specgate-fixtures`).
    cargo_name: String,
    /// Rust identifier form (hyphens → underscores, e.g., `specgate_fixtures`).
    rust_ident: String,
    /// Module name declared in lib.rs (e.g., `cross_dep`), or `None` when the
    /// contributing source is the crate root (`src/lib.rs`) — i.e. the op is a
    /// `pub` item at the crate root and is linked as `use <crate> as fut;`.
    module_name: Option<String>,
    /// Path to the fixture crate root.
    path: PathBuf,
}

/// Build the linked-crate info for one contributing module. Returns `None` only
/// when the target has no `[package] name` (so it cannot be linked at all).
///
/// This does NOT require the module to be `pub mod`-declared: a module that is
/// not publicly declared still yields a `use <crate>::<module> as fut;` link
/// that fails to compile, surfacing the target's own compiler diagnostics
/// (e.g. a syntax error in the source) as a `"source failed to compile"` error.
/// Whether the module is a legitimate public path is decided separately by
/// [`module_publicly_linkable`], which gates the clean reachability diagnostic.
fn crate_info_for(fixture_pkg_root: &Path, module_name: &str) -> Option<FixtureCrateInfo> {
    let cargo_toml = fixture_pkg_root.join("Cargo.toml");
    let text = std::fs::read_to_string(&cargo_toml).ok()?;
    let cargo_name = parse_cargo_name(&text)?;
    let rust_ident = cargo_name.replace('-', "_");

    // Crate-root op (`src/lib.rs`): the op is a public item at the crate root,
    // so link the crate itself with no submodule (`use <crate> as fut;`).
    let module = (module_name != "lib").then(|| module_name.to_string());

    Some(FixtureCrateInfo {
        cargo_name,
        rust_ident,
        module_name: module,
        path: fixture_pkg_root.to_path_buf(),
    })
}

/// Whether `module_name` is reachable through the crate's public path: it is the
/// crate root (`lib`), or `src/lib.rs` has an *active* `pub mod <module_name>;`
/// declaration (a commented-out declaration does not count).
pub(crate) fn module_publicly_linkable(fixture_pkg_root: &Path, module_name: &str) -> bool {
    if module_name == "lib" {
        return true;
    }
    let Ok(lib_text) = std::fs::read_to_string(fixture_pkg_root.join("src").join("lib.rs")) else {
        return false;
    };
    let decl = format!("pub mod {module_name};");
    lib_text.lines().any(|line| {
        let l = line.trim_start();
        if l.starts_with("//") {
            return false;
        }
        // Ignore any trailing line comment before matching the declaration.
        l.split("//").next().unwrap_or(l).contains(&decl)
    })
}

/// A human-friendly label for the target crate (its `[package] name`, falling
/// back to the package root path).
pub(crate) fn crate_label(package_root: &Path) -> String {
    std::fs::read_to_string(package_root.join("Cargo.toml"))
        .ok()
        .and_then(|t| parse_cargo_name(&t))
        .unwrap_or_else(|| to_cargo_path(package_root))
}

/// Build the link info for every contributing module. Each module lives in the
/// same target crate, linked as its own Cargo dependency.
///
/// # Errors
///
/// Returns an error only when the target has no `[package] name` and therefore
/// cannot be linked at all. (Public-reachability of individual operations is
/// enforced earlier, in the harness pre-flight.)
pub(crate) fn resolve_fixture_crates(package_root: &Path, fixture_srcs: &[PathBuf]) -> Result<Vec<FixtureCrateInfo>, String> {
    let module_names: Vec<String> = fixture_srcs
        .iter()
        .map(|p| p.file_stem().and_then(|s| s.to_str()).unwrap_or("fixture").to_string())
        .collect();

    let mut resolved: Vec<FixtureCrateInfo> = Vec::new();
    for m in &module_names {
        match crate_info_for(package_root, m) {
            Some(info) => resolved.push(info),
            None => {
                return Err(format!(
                    "target crate at '{}' has no `[package] name`; cannot link the target",
                    to_cargo_path(package_root)
                ));
            }
        }
    }

    Ok(resolved)
}

fn parse_cargo_name(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
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

/// Convert a path to a forward-slash string suitable for Cargo.toml.
/// Strips the Windows extended path prefix `\\?\` if present.
fn to_cargo_path(p: &Path) -> String {
    let s = p.display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

pub fn generate(scratch_dir: &Path, config: &GenerateConfig) -> std::io::Result<GeneratedProject> {
    std::fs::create_dir_all(scratch_dir.join("src"))?;
    let trace_file = scratch_dir.join("traces.json");

    let annotations_path = config.workspace_root.join("crates/specgate");
    let runtime_path = config.workspace_root.join("crates/specgate-runtime");
    let macros_path = config.workspace_root.join("crates/specgate-macros");
    let harness_path = config.workspace_root.join("crates/specgate-harness");

    // Link-only: the target crate is always a path dependency. Every
    // contributing module lives in the same crate, so add the dependency once.
    let crates = config.fixture_crates;
    let fixture_dep = crates
        .first()
        .map(|fc| format!("\n{} = {{ path = \"{}\" }}", fc.cargo_name, to_cargo_path(&fc.path)))
        .unwrap_or_default();

    let specgate_deps = if config.is_local {
        format!(
            "specgate = {{ path = \"{ann}\" }}\nspecgate-harness = {{ path = \"{harness}\" }}",
            ann = to_cargo_path(&annotations_path),
            harness = to_cargo_path(&harness_path),
        )
    } else {
        format!(
            "specgate = \"{ver}\"\nspecgate-harness = \"{ver}\"",
            ver = env!("CARGO_PKG_VERSION"),
        )
    };

    // Only pull an async runtime into the runner when a case actually awaits.
    // Non-async runners stay dependency-identical to before.
    let runtime_dep = if config.needs_async {
        match config.runtime {
            Runtime::Smol => "\nsmol = \"2\"",
            Runtime::Tokio => "\ntokio = { version = \"1\", features = [\"rt\", \"time\"] }",
        }
    } else {
        ""
    };

    let manifest = format!(
        r#"[package]
name = "sg-runner"
version = "0.0.1"
edition = "2024"

[[bin]]
name = "sg-runner"
path = "src/main.rs"

[dependencies]
{specgate_deps}
serde_yaml = "0.9"{fixture_dep}{runtime_dep}

[workspace]
"#,
    );
    let _ = runtime_path;
    let _ = macros_path;
    std::fs::write(scratch_dir.join("Cargo.toml"), manifest)?;

    // Seed the tmp project's Cargo.lock from the parent workspace so cargo
    // doesn't need to consult crates.io (the env may have it blocked).
    let parent_lock = config.workspace_root.join("Cargo.lock");
    let tmp_lock = scratch_dir.join("Cargo.lock");
    if parent_lock.exists() {
        let _ = std::fs::copy(&parent_lock, &tmp_lock);
    }

    let main_rs = render_main(
        config.spec,
        config.cases_to_run,
        config.annotated,
        &trace_file,
        config.needs_async.then_some(config.runtime),
        crates,
    );
    std::fs::write(scratch_dir.join("src").join("main.rs"), main_rs)?;

    Ok(GeneratedProject {
        crate_dir: scratch_dir.to_path_buf(),
        trace_file,
    })
}

fn render_main(
    spec: &Spec,
    cases_to_run: &[&Case],
    annotated: &AnnotatedSource,
    trace_out: &Path,
    async_runtime: Option<Runtime>,
    fixture_crates: &[FixtureCrateInfo],
) -> String {
    let mut out = String::new();
    let needs_async = async_runtime.is_some();
    out.push_str("#![allow(unused, unused_mut, unused_variables, dead_code, clippy::all)]\n");
    // The runner is glue-only — no target source is inlined, so it is
    // absolutely unsafe-free. Use `forbid` (cannot be overridden) rather than
    // `deny`. The real target crate is linked as its own dependency and keeps
    // whatever unsafe policy it defines.
    out.push_str("#![forbid(unsafe_code)]\n");
    out.push_str("use specgate::{TraceEvent, Value, take_traces, reset, set_mock, SpecEvent};\n");
    out.push_str("use std::collections::HashMap;\n");

    match fixture_crates {
        // Single module: alias the linked public path directly as `fut`.
        // A submodule op -> `use <crate>::<module> as fut;`; a crate-root op
        // (defined in the crate's `lib.rs`) -> `use <crate> as fut;`.
        [fc] => match &fc.module_name {
            Some(module) => writeln!(out, "use {}::{} as fut;", fc.rust_ident, module).expect("fmt"),
            None => writeln!(out, "use {} as fut;", fc.rust_ident).expect("fmt"),
        },
        // Multiple modules (operations split across files): re-export each
        // module's public items into a synthetic `fut` module.
        crates => {
            out.push_str("mod fut {\n");
            for fc in crates {
                match &fc.module_name {
                    Some(module) => writeln!(out, "    pub use ::{}::{}::*;", fc.rust_ident, module).expect("fmt"),
                    None => writeln!(out, "    pub use ::{}::*;", fc.rust_ident).expect("fmt"),
                }
            }
            out.push_str("}\n");
        }
    }
    out.push_str("use fut::*;\n");
    out.push('\n');
    out.push_str("fn panic_msg(e: &Box<dyn std::any::Any + Send>) -> String {\n");
    out.push_str("    if let Some(s) = e.downcast_ref::<String>() { return s.clone(); }\n");
    out.push_str("    if let Some(s) = e.downcast_ref::<&'static str>() { return s.to_string(); }\n");
    out.push_str("    \"panic\".to_string()\n");
    out.push_str("}\n\n");

    if needs_async {
        out.push_str(ASYNC_CATCH_UNWIND);
    }

    out.push_str("fn main() {\n");
    out.push_str("    let out_path = std::env::args().nth(1).expect(\"missing output path\");\n");
    out.push_str("    let mut all: std::collections::BTreeMap<String, Vec<TraceEvent>> = std::collections::BTreeMap::new();\n");

    // Async runners drive every case inside ONE top-level runtime entry, so
    // reactor-backed awaits (tokio timers/IO, smol timers) make progress. Sync
    // runners keep the plain body (byte-identical to the pre-async output).
    if let Some(rt) = async_runtime {
        let entry = match rt {
            Runtime::Smol => "    smol::block_on(async {\n",
            Runtime::Tokio => "    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {\n",
        };
        out.push_str(entry);
    }

    for case in cases_to_run {
        writeln!(out, "    // ---- case: {} ----", case.name).expect("fmt");
        out.push_str("    {\n");
        out.push_str("        reset();\n");
        render_case(&mut out, case, spec, annotated);
        writeln!(out, "        all.insert({:?}.to_string(), take_traces());", case.name).expect("fmt");
        out.push_str("    }\n");
    }

    if needs_async {
        out.push_str("    });\n");
    }

    write!(
        out,
        "    let s = serde_json_lite_to_string(&all);\n    std::fs::write({:?}, s).expect(\"write traces\");\n",
        trace_out.display().to_string()
    )
    .expect("fmt");
    out.push_str("}\n\n");

    // Inline a tiny JSON serializer to avoid pulling serde_json into the
    // generated crate. We only need to emit our own TraceEvent shape.
    out.push_str(JSON_HELPER);

    out
}

/// A future-aware `catch_unwind` used to isolate panics from an awaited op
/// without letting a `std::panic::catch_unwind` span an `.await` (which is
/// unsound). Each poll is wrapped so a panic during polling is captured and
/// surfaced as `$fault`, matching the sync path's behavior. Self-contained so
/// the runner needs no extra `futures*` dependency.
///
/// Safe / std-only: built on `std::future::poll_fn` (stable since 1.64) plus
/// `Box::pin`, so it contains no `unsafe` and pulls in no `futures*` crate. The
/// real `cx`/waker is passed through to the inner future on every poll (so
/// reactor-backed futures still get woken), panics are captured per-poll, and
/// the future is dropped on panic (never re-polled).
const ASYNC_CATCH_UNWIND: &str = r"
async fn sg_catch_unwind<F: ::std::future::Future>(fut: F)
    -> ::std::result::Result<F::Output, ::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>>
{
    use ::std::task::Poll;
    use ::std::panic::{catch_unwind, AssertUnwindSafe};
    let mut fut = ::std::boxed::Box::pin(fut);
    ::std::future::poll_fn(move |cx| {
        match catch_unwind(AssertUnwindSafe(|| fut.as_mut().poll(cx))) {
            Ok(Poll::Pending)  => Poll::Pending,
            Ok(Poll::Ready(v)) => Poll::Ready(Ok(v)),
            Err(e)             => Poll::Ready(Err(e)),
        }
    }).await
}
";

const JSON_HELPER: &str = r#"
fn esc_str(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
}

fn value_to_json(v: &Value, o: &mut String) {
    match v {
        Value::String(s) => esc_str(s, o),
        Value::Integer(i) => o.push_str(&i.to_string()),
        Value::Float(x) => o.push_str(&x.to_string()),
        Value::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
        Value::List(items) => {
            o.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 { o.push(','); }
                value_to_json(it, o);
            }
            o.push(']');
        }
        Value::Set(items) => {
            o.push('[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 { o.push(','); }
                value_to_json(it, o);
            }
            o.push(']');
        }
        Value::Map(m) => {
            o.push('{');
            let mut first = true;
            for (k, vv) in m.iter() {
                if !first { o.push(','); }
                first = false;
                esc_str(k, o);
                o.push(':');
                value_to_json(vv, o);
            }
            o.push('}');
        }
    }
}

fn serde_json_lite_to_string(map: &std::collections::BTreeMap<String, Vec<TraceEvent>>) -> String {
    let mut s = String::from("{");
    let mut first = true;
    for (k, v) in map.iter() {
        if !first { s.push(','); }
        first = false;
        esc_str(k, &mut s);
        s.push(':');
        s.push('[');
        let mut f2 = true;
        for ev in v {
            if !f2 { s.push(','); }
            f2 = false;
            match ev {
                TraceEvent::Event { name, value } => {
                    s.push_str("{\"kind\":\"Event\",\"name\":");
                    esc_str(name, &mut s);
                    s.push_str(",\"value\":");
                    value_to_json(value, &mut s);
                    s.push('}');
                }
                TraceEvent::Run { operation } => {
                    s.push_str("{\"kind\":\"Run\",\"operation\":");
                    esc_str(operation, &mut s);
                    s.push('}');
                }
            }
        }
        s.push(']');
    }
    s.push('}');
    s
}
"#;

fn render_case(out: &mut String, case: &Case, spec: &Spec, annotated: &AnnotatedSource) {
    // Mock table: any input key that's a mapping is treated as a mock table
    // named after the key. (Convention from fixtures.)
    for (k, v) in &case.inputs {
        if let Value::Mapping(m) = v {
            writeln!(out, "        set_mock({k:?}, &[").expect("fmt");
            for (mk, mv) in m {
                if let (Some(ks), Some(vs)) = (mk.as_str(), mv.as_str()) {
                    writeln!(out, "            ({ks:?}, {vs:?}),").expect("fmt");
                }
            }
            out.push_str("        ]);\n");
        }
    }

    // Setups: resolve which setups construct the operation's receiver/params,
    // build them, and emit their initial SpecEvent fields. Resolution errors
    // are surfaced earlier (pre-flight in run_group), so unwrap is safe here.
    let case_ops: Vec<&str> = if !case.steps.is_empty() {
        case.steps.iter().map(String::as_str).collect()
    } else if let Some(op) = case.operation.as_deref() {
        vec![op]
    } else {
        vec![]
    };
    let bindings = annotated.resolve_case(&case_ops).unwrap_or_default();
    for b in &bindings {
        let args = render_construct_args(&b.params, &b.target, &case.inputs);
        writeln!(out, "        let mut {} = fut::{}({args});", b.var, b.fn_ident).expect("fmt");
        let ret_ty = annotated
            .setups
            .iter()
            .find(|s| s.sig.fn_ident == b.fn_ident)
            .map(|s| s.sig.return_type.trim().to_string())
            .unwrap_or_default();
        let derives_event = annotated.spec_event_structs.contains(ret_ty.as_str());
        match &b.target {
            crate::scan::SetupTarget::Receiver => {
                if derives_event {
                    writeln!(out, "        SpecEvent::emit_fields(&{}, None);", b.var).expect("fmt");
                }
            }
            crate::scan::SetupTarget::Param(p) => {
                if derives_event {
                    writeln!(out, "        SpecEvent::emit_fields(&{}, Some({p:?}));", b.var).expect("fmt");
                }
            }
            crate::scan::SetupTarget::SideEffect => {
                writeln!(out, "        let _ = &{};", b.var).expect("fmt");
            }
        }
    }

    // Steps or single operation.
    let ops: Vec<&str> = if !case.steps.is_empty() {
        case.steps.iter().map(String::as_str).collect()
    } else if let Some(op) = case.operation.as_deref() {
        vec![op]
    } else {
        vec![]
    };

    let is_steps = !case.steps.is_empty();
    for (i, op) in ops.iter().enumerate() {
        let op = *op;
        let decl = annotated.operations.get(op);
        let op_defaults = spec.op_input_defaults.get(op);
        // A multi-step case may carry per-step `inputs:`; use them for that
        // step's call (free-function steps supply their own arguments). Fall
        // back to the shared case-level inputs when a step omits them (the
        // state-machine step shape, where inputs come from the receiver).
        let inputs = if is_steps {
            case.step_inputs.get(i).filter(|m| !m.is_empty()).unwrap_or(&case.inputs)
        } else {
            &case.inputs
        };
        let call = render_op_call(op, decl, inputs, &bindings, annotated, op_defaults);
        let is_async = spec.async_ops.contains(op);
        // The annotated operation self-emits its full trace (`$run`, input
        // echoes, and `$result`/field events) via `#[spec_operation]`, so the
        // runner only needs to invoke it and discard the value.
        out.push_str("        {\n");
        if is_async {
            // Async op: await directly inside the top-level runtime entry, with
            // a future-aware catch_unwind so one panicking op can't abort the
            // other cases (a plain catch_unwind can't span an `.await`).
            write!(out,
                "            let __r = sg_catch_unwind(async {{\n                let __sg_ret = {call}.await;\n                let _ = __sg_ret;\n            }}).await;\n"
            ).expect("fmt");
        } else {
            write!(out,
                "            let __r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n                let __sg_ret = {call};\n                let _ = __sg_ret;\n            }}));\n"
            ).expect("fmt");
        }
        out.push_str("            if let Err(__e) = __r {\n");
        out.push_str("                let msg = panic_msg(&__e);\n                specgate::emit_event(\"$fault\", &msg);\n");
        out.push_str("            }\n");
        out.push_str("        }\n");
    }
}

/// Render the construction arguments for a setup call. Values come from the
/// case inputs, routed by the setup's parameter names. When one setup fills a
/// named parameter (via `fills`), each construction input may be given per fill
/// as a flat `<param>_<fills>` input; otherwise the bare `<param>` is used.
fn render_construct_args(params: &[(String, String)], target: &crate::scan::SetupTarget, inputs: &BTreeMap<String, Value>) -> String {
    let role: Option<&str> = if let crate::scan::SetupTarget::Param(p) = target {
        Some(p.as_str())
    } else {
        None
    };
    let mut parts = Vec::new();
    for (name, ty) in params {
        let v = role.and_then(|r| inputs.get(&format!("{name}_{r}"))).or_else(|| inputs.get(name));
        parts.push(value_to_rust(v, ty));
    }
    parts.join(", ")
}

fn render_op_call(
    op_name: &str,
    decl: Option<&OpDecl>,
    inputs: &BTreeMap<String, Value>,
    bindings: &[crate::scan::SetupBinding],
    annotated: &AnnotatedSource,
    op_defaults: Option<&BTreeMap<String, Value>>,
) -> String {
    let Some(decl) = decl else {
        return format!("fut::{op_name}()");
    };

    // Method: the receiver is the setup binding that targets the receiver.
    if decl.takes_self {
        let recv_var = bindings
            .iter()
            .find(|b| matches!(b.target, crate::scan::SetupTarget::Receiver))
            .map_or_else(|| "/* missing receiver */".to_string(), |b| b.var.clone());
        let args = render_op_args(decl, inputs, bindings, op_defaults);
        return format!("{recv_var}.{}({args})", decl.sig.fn_ident);
    }

    let _ = annotated;
    let args = render_op_args(decl, inputs, bindings, op_defaults);
    format!("fut::{}({args})", decl.sig.fn_ident)
}

fn render_op_args(
    decl: &OpDecl,
    inputs: &BTreeMap<String, Value>,
    bindings: &[crate::scan::SetupBinding],
    op_defaults: Option<&BTreeMap<String, Value>>,
) -> String {
    let mut parts = Vec::new();
    for (p, ty) in &decl.sig.params {
        // If a setup binding fills this parameter, pass its variable.
        if let Some(b) = bindings
            .iter()
            .find(|b| matches!(&b.target, crate::scan::SetupTarget::Param(n) if n == p))
        {
            let prefix = if ty.starts_with("&mut") {
                "&mut "
            } else if ty.starts_with('&') {
                "&"
            } else {
                ""
            };
            parts.push(format!("{prefix}{}", b.var));
            continue;
        }
        // Case-provided input wins; otherwise fall back to a declared default
        // for this input, materialized through the same `value_to_rust` path so
        // scalar and complex/named-type defaults both work.
        let v = inputs.get(p).or_else(|| op_defaults.and_then(|d| d.get(p)));
        parts.push(value_to_rust(v, ty));
    }
    parts.join(", ")
}

fn value_to_rust(v: Option<&Value>, ty: &str) -> String {
    let ty = ty.trim();
    let Some(v) = v else {
        return "Default::default()".into();
    };
    let ty_norm = ty.trim_start_matches('&').trim_start_matches("mut ").trim();

    // Option<T> → None or Some(inner)
    if let Some(inner) = strip_option(ty_norm) {
        return match v {
            Value::Null => "None".into(),
            _ => format!("Some({})", value_to_rust(Some(v), inner)),
        };
    }

    // &[T] slices — keep inline approach for backward compat
    if ty_norm.starts_with('[') || ty.starts_with("&[") {
        let elem_ty = inner_ty(ty_norm);
        if let Value::Sequence(seq) = v {
            let elements: Vec<String> = seq
                .iter()
                .map(|e| value_to_rust(Some(e), elem_ty.as_deref().unwrap_or("i32")))
                .collect();
            return format!("&[{}][..]", elements.join(", "));
        }
        return "Default::default()".into();
    }

    match v {
        Value::Number(n) => {
            // Suffix int with type.
            if ty_norm.starts_with('i') || ty_norm.starts_with('u') || ty_norm == "f32" || ty_norm == "f64" {
                format!("{n}{ty_norm}")
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::String(s) => {
            if ty_norm == "String" {
                format!("{s:?}.to_string()")
            } else if ty_norm == "&str" || ty_norm == "str" {
                format!("{s:?}")
            } else {
                // Named type passed as a string scalar (e.g. "Point") → serde_yaml
                yaml_deser(v, ty_norm)
            }
        }
        // Sequences and mappings: always deserialize via serde_yaml
        Value::Sequence(_) | Value::Mapping(_) => yaml_deser(v, ty_norm),
        Value::Null => "Default::default()".into(),
        Value::Tagged(t) => value_to_rust(Some(&t.value), ty),
    }
}

/// Emit a deserialization expression for a named type from the spec value.
///
/// Uses `singleton_map_recursive` so externally-tagged enums are read in the
/// canonical `{ Variant: data }` form (matching the spec's trace format) rather
/// than `serde_yaml`'s default `!Variant` tag syntax. Structs, maps, sequences,
/// and scalars pass through unchanged.
fn yaml_deser(v: &Value, ty: &str) -> String {
    let yaml_str = serde_yaml::to_string(v).unwrap_or_else(|_| "~\n".to_string());
    format!(
        "{{ let __sg_v: {ty} = serde_yaml::with::singleton_map_recursive::deserialize(serde_yaml::Deserializer::from_str({yaml_str:?})).unwrap(); __sg_v }}"
    )
}

/// Extract the inner type from `Option<T>`.
fn strip_option(ty: &str) -> Option<&str> {
    for prefix in &["Option<", "::std::option::Option<", "std::option::Option<"] {
        if let Some(rest) = ty.strip_prefix(prefix) {
            return rest.strip_suffix('>').map(str::trim);
        }
    }
    None
}

fn inner_ty(ty: &str) -> Option<String> {
    // &[T] or [T] or Vec<T>
    let ty = ty.trim();
    if let Some(rest) = ty.strip_prefix('[') {
        return rest.strip_suffix(']').map(|s| s.trim().to_string());
    }
    if let Some(rest) = ty.strip_prefix("Vec<") {
        return rest.strip_suffix('>').map(|s| s.trim().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn empty_spec() -> Spec {
        Spec {
            binding_path: None,
            target: None,
            cases: vec![],
            async_ops: BTreeSet::new(),
            op_input_defaults: BTreeMap::new(),
        }
    }

    fn empty_annotated() -> AnnotatedSource {
        AnnotatedSource {
            setups: Vec::new(),
            operations: BTreeMap::new(),
            spec_event_structs: BTreeSet::new(),
            spec_event_enums: BTreeSet::new(),
        }
    }

    #[test]
    fn generated_manifest_uses_version_deps_when_not_local() {
        let scratch = tempfile::tempdir().unwrap();
        let spec = empty_spec();
        let annotated = empty_annotated();
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        let config = GenerateConfig {
            spec: &spec,
            cases_to_run: &[],
            annotated: &annotated,
            workspace_root: &workspace_root,
            needs_async: false,
            runtime: Runtime::Smol,
            fixture_crates: &[],
            is_local: false,
        };

        let result = generate(scratch.path(), &config);
        assert!(result.is_ok(), "generate failed: {:?}", result.err());

        let manifest = std::fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains(&format!("specgate = \"{}\"", env!("CARGO_PKG_VERSION"))),
            "manifest should contain version dep for specgate, got:\n{manifest}"
        );
        assert!(
            !manifest.contains("{ path ="),
            "manifest should NOT contain path deps when is_local=false, got:\n{manifest}"
        );
    }

    #[test]
    fn generated_manifest_uses_path_deps_when_local() {
        let scratch = tempfile::tempdir().unwrap();
        let spec = empty_spec();
        let annotated = empty_annotated();
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        let config = GenerateConfig {
            spec: &spec,
            cases_to_run: &[],
            annotated: &annotated,
            workspace_root: &workspace_root,
            needs_async: false,
            runtime: Runtime::Smol,
            fixture_crates: &[],
            is_local: true,
        };

        let result = generate(scratch.path(), &config);
        assert!(result.is_ok(), "generate failed: {:?}", result.err());

        let manifest = std::fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("path ="),
            "manifest should contain path deps when is_local=true, got:\n{manifest}"
        );
    }

    fn op_decl(params: &[(&str, &str)]) -> OpDecl {
        OpDecl {
            sig: crate::scan::FnSig {
                fn_ident: "scale".into(),
                params: params.iter().map(|(n, t)| ((*n).to_string(), (*t).to_string())).collect(),
                return_type: "i32".into(),
            },
            method_of: None,
            takes_self: false,
            is_pub: true,
        }
    }

    #[test]
    fn render_op_args_uses_declared_default_when_case_omits_input() {
        let decl = op_decl(&[("value", "i32"), ("factor", "i32")]);
        let mut inputs = BTreeMap::new();
        inputs.insert("value".to_string(), Value::Number(5.into()));
        let mut defaults = BTreeMap::new();
        defaults.insert("factor".to_string(), Value::Number(2.into()));

        let args = render_op_args(&decl, &inputs, &[], Some(&defaults));
        assert_eq!(args, "5i32, 2i32");
    }

    #[test]
    fn render_op_args_case_value_overrides_default() {
        let decl = op_decl(&[("value", "i32"), ("factor", "i32")]);
        let mut inputs = BTreeMap::new();
        inputs.insert("value".to_string(), Value::Number(5.into()));
        inputs.insert("factor".to_string(), Value::Number(3.into()));
        let mut defaults = BTreeMap::new();
        defaults.insert("factor".to_string(), Value::Number(2.into()));

        let args = render_op_args(&decl, &inputs, &[], Some(&defaults));
        assert_eq!(args, "5i32, 3i32");
    }

    #[test]
    fn render_op_args_no_default_falls_back_to_default_trait() {
        let decl = op_decl(&[("value", "i32"), ("factor", "i32")]);
        let mut inputs = BTreeMap::new();
        inputs.insert("value".to_string(), Value::Number(5.into()));

        // No declared default and case omits `factor` → prior behavior.
        let args = render_op_args(&decl, &inputs, &[], None);
        assert_eq!(args, "5i32, Default::default()");
    }

    #[test]
    fn render_op_args_complex_default_materializes_via_yaml_deser() {
        let decl = op_decl(&[("base", "i32"), ("by", "Offset")]);
        let mut inputs = BTreeMap::new();
        inputs.insert("base".to_string(), Value::Number(5.into()));
        let default_by: Value = serde_yaml::from_str("{ dx: 1, dy: 1 }").unwrap();
        let mut defaults = BTreeMap::new();
        defaults.insert("by".to_string(), default_by);

        let args = render_op_args(&decl, &inputs, &[], Some(&defaults));
        // The complex default flows through the same `value_to_rust` path used
        // for case-provided complex inputs (serde_yaml deserialization).
        assert!(args.starts_with("5i32, "), "args={args}");
        assert!(
            args.contains("yaml_deser") || args.contains("Offset"),
            "complex default not materialized: {args}"
        );
    }
}
