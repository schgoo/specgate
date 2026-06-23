//! Generate a temporary Cargo project that compiles + executes a fixture
//! against the spec's cases and writes a JSON trace to disk.

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
    pub fixture_pkg_root: Option<&'a Path>,
    pub is_local: bool,
}

/// Information about the fixture crate for use as a Cargo dependency.
struct FixtureCrateInfo {
    /// The `name` field from the fixture crate's Cargo.toml (e.g., `specgate-fixtures`).
    cargo_name: String,
    /// Rust identifier form (hyphens → underscores, e.g., `specgate_fixtures`).
    rust_ident: String,
    /// Module name declared in lib.rs (e.g., `cross_dep`).
    module_name: String,
    /// Path to the fixture crate root.
    path: PathBuf,
}

/// Try to resolve the fixture crate dependency info. Returns `Some` only when:
/// 1. `fixture_pkg_root` has a `Cargo.toml` with a `[package] name`
/// 2. `fixture_pkg_root/src/lib.rs` contains `pub mod <module_name>;`
fn resolve_fixture_crate(fixture_pkg_root: &Path, module_name: &str) -> Option<FixtureCrateInfo> {
    let cargo_toml = fixture_pkg_root.join("Cargo.toml");
    let text = std::fs::read_to_string(&cargo_toml).ok()?;
    let cargo_name = parse_cargo_name(&text)?;
    let rust_ident = cargo_name.replace('-', "_");

    let lib_rs = fixture_pkg_root.join("src").join("lib.rs");
    let lib_text = std::fs::read_to_string(&lib_rs).ok()?;
    let decl = format!("pub mod {module_name};");
    if !lib_text.contains(&decl) {
        return None;
    }

    Some(FixtureCrateInfo {
        cargo_name,
        rust_ident,
        module_name: module_name.to_string(),
        path: fixture_pkg_root.to_path_buf(),
    })
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

pub fn generate(scratch_dir: &Path, fixture_src: &Path, config: &GenerateConfig) -> std::io::Result<GeneratedProject> {
    std::fs::create_dir_all(scratch_dir.join("src"))?;
    let trace_file = scratch_dir.join("traces.json");

    let annotations_path = config.workspace_root.join("crates/specgate");
    let runtime_path = config.workspace_root.join("crates/specgate-runtime");
    let macros_path = config.workspace_root.join("crates/specgate-macros");
    let harness_path = config.workspace_root.join("crates/specgate-harness");

    // Determine the fixture module name from the source file stem.
    let module_name = fixture_src.file_stem().and_then(|s| s.to_str()).unwrap_or("fixture").to_string();

    // Try to use the fixture crate as a path dependency when possible.
    let fixture_crate = config.fixture_pkg_root.and_then(|root| resolve_fixture_crate(root, &module_name));

    let fixture_dep = if let Some(ref fc) = fixture_crate {
        format!("\n{} = {{ path = \"{}\" }}", fc.cargo_name, to_cargo_path(&fc.path))
    } else {
        String::new()
    };

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
serde_yaml = "0.9"{fixture_dep}

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
        fixture_src,
        config.spec,
        config.cases_to_run,
        config.annotated,
        &trace_file,
        config.needs_async,
        fixture_crate.as_ref(),
    )?;
    std::fs::write(scratch_dir.join("src").join("main.rs"), main_rs)?;

    Ok(GeneratedProject {
        crate_dir: scratch_dir.to_path_buf(),
        trace_file,
    })
}

fn render_main(
    fixture_src: &Path,
    spec: &Spec,
    cases_to_run: &[&Case],
    annotated: &AnnotatedSource,
    trace_out: &Path,
    needs_async: bool,
    fixture_crate: Option<&FixtureCrateInfo>,
) -> std::io::Result<String> {
    let mut out = String::new();
    out.push_str("#![allow(unused, unused_mut, unused_variables, dead_code, clippy::all)]\n");
    out.push_str("use specgate::{TraceEvent, Value, take_traces, reset, set_mock, SpecEvent};\n");
    out.push_str("use std::collections::HashMap;\n");

    if let Some(fc) = fixture_crate {
        // Alias the fixture module as `fut` so call sites work uniformly.
        writeln!(out, "use {}::{} as fut;", fc.rust_ident, fc.module_name).expect("fmt");
    } else {
        let abs = std::fs::canonicalize(fixture_src)?;
        let abs_str = abs.display().to_string();
        let abs_str = abs_str.strip_prefix(r"\\?\").unwrap_or(&abs_str);
        writeln!(out, "#[path = \"{}\"] mod fut;", abs_str.replace('\\', "\\\\")).expect("fmt");
    }
    out.push_str("use fut::*;\n");
    out.push('\n');
    out.push_str("fn panic_msg(e: &Box<dyn std::any::Any + Send>) -> String {\n");
    out.push_str("    if let Some(s) = e.downcast_ref::<String>() { return s.clone(); }\n");
    out.push_str("    if let Some(s) = e.downcast_ref::<&'static str>() { return s.to_string(); }\n");
    out.push_str("    \"panic\".to_string()\n");
    out.push_str("}\n\n");

    if needs_async {
        out.push_str(ASYNC_BLOCK_ON);
    }

    out.push_str("fn main() {\n");
    out.push_str("    let out_path = std::env::args().nth(1).expect(\"missing output path\");\n");
    out.push_str("    let mut all: std::collections::BTreeMap<String, Vec<TraceEvent>> = std::collections::BTreeMap::new();\n");

    for case in cases_to_run {
        writeln!(out, "    // ---- case: {} ----", case.name).expect("fmt");
        out.push_str("    {\n");
        out.push_str("        reset();\n");
        render_case(&mut out, case, spec, annotated);
        writeln!(out, "        all.insert({:?}.to_string(), take_traces());", case.name).expect("fmt");
        out.push_str("    }\n");
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

    Ok(out)
}

/// A minimal no-op-waker `block_on`. Sufficient for fixture async fns that
/// don't yield to a real reactor — they complete on the first poll.
const ASYNC_BLOCK_ON: &str = r"
fn sg_block_on<F: ::std::future::Future>(fut: F) -> F::Output {
    use ::std::pin::pin;
    use ::std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    const VT: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(::std::ptr::null(), &VT),
        |_| {},
        |_| {},
        |_| {},
    );
    let raw = RawWaker::new(::std::ptr::null(), &VT);
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
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

    for op in ops {
        let decl = annotated.operations.get(op);
        let mut call = render_op_call(op, decl, &case.inputs, &bindings, annotated);
        if spec.async_ops.contains(op) {
            call = format!("sg_block_on({call})");
        }
        let return_type = decl.map(|d| d.sig.return_type.trim().to_string()).unwrap_or_default();
        let post_emit = build_post_emit(&return_type, &annotated.spec_event_structs, &annotated.spec_event_enums);
        out.push_str("        {\n");
        write!(out,
            "            let __r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n                let __sg_ret = {call};\n                {post_emit}\n            }}));\n"
        ).expect("fmt");
        out.push_str("            if let Err(__e) = __r {\n");
        out.push_str("                let msg = panic_msg(&__e);\n                specgate::emit_event(\"$fault\", &msg);\n");
        out.push_str("            }\n");
        out.push_str("        }\n");
    }
}

/// Emit Rust source for post-call return handling based on the operation's
/// declared return type. Produces statements that consume `__sg_ret`.
fn build_post_emit(
    return_type: &str,
    spec_event_structs: &std::collections::BTreeSet<String>,
    spec_event_enums: &std::collections::BTreeSet<String>,
) -> String {
    let rt = return_type.trim();
    if rt.is_empty() || rt == "()" {
        return "let _ = __sg_ret;".to_string();
    }
    if rt.starts_with("Result<") || rt.starts_with("::std::result::Result<") || rt.starts_with("std::result::Result<") {
        return r#"
            match &__sg_ret {
                Ok(__sg_v) => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("Ok".to_string(), specgate::__rt::ToSpecValue::to_spec_value(__sg_v));
                    specgate::emit_event_v("$result", specgate::Value::Map(__sg_m));
                }
                Err(__sg_e) => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("Err".to_string(), specgate::Value::String(format!("{}", __sg_e)));
                    specgate::emit_event_v("$result", specgate::Value::Map(__sg_m));
                }
            }
            let _ = __sg_ret;
        "#
        .to_string();
    }
    if rt.starts_with("Option<") || rt.starts_with("::std::option::Option<") || rt.starts_with("std::option::Option<") {
        return r#"
            match &__sg_ret {
                Some(__sg_v) => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("Some".to_string(), specgate::__rt::ToSpecValue::to_spec_value(__sg_v));
                    specgate::emit_event_v("$result", specgate::Value::Map(__sg_m));
                }
                None => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("None".to_string(), specgate::Value::Map(::std::collections::BTreeMap::new()));
                    specgate::emit_event_v("$result", specgate::Value::Map(__sg_m));
                }
            }
            let _ = __sg_ret;
        "#
        .to_string();
    }
    // SpecEvent-derived struct: emit each annotated field and the full
    // structured $result via ToSpecValue.
    let bare = rt.trim_start_matches('&').trim_start_matches("mut ").trim();
    let head = bare.split(['<', ' ']).next().unwrap_or(bare);
    // Enum returns: emit ONLY the structured $result (tagged variant map),
    // no dotted field events.
    if spec_event_enums.contains(head) {
        return r#"
            specgate::emit_event_v(
                "$result",
                specgate::__rt::ToSpecValue::to_spec_value(&__sg_ret),
            );
            let _ = __sg_ret;
        "#
        .to_string();
    }
    if spec_event_structs.contains(head) {
        return r#"
            specgate::SpecEvent::emit_fields(&__sg_ret, None);
            specgate::emit_event_v(
                "$result",
                specgate::__rt::ToSpecValue::to_spec_value(&__sg_ret),
            );
            let _ = __sg_ret;
        "#
        .to_string();
    }
    // Known collection types → use ToSpecValue for structured emission.
    let is_collection = matches!(head, "Vec" | "BTreeMap" | "HashMap" | "BTreeSet" | "HashSet") || bare.starts_with('[');
    if is_collection {
        return r#"
            specgate::emit_event_v(
                "$result",
                specgate::__rt::ToSpecValue::to_spec_value(&__sg_ret),
            );
            let _ = __sg_ret;
        "#
        .to_string();
    }
    // Default: emit $result via Display.
    r#"
            specgate::emit_event("$result", &format!("{}", __sg_ret));
            let _ = __sg_ret;
        "#
    .to_string()
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
        let args = render_op_args(decl, inputs, bindings);
        return format!("{recv_var}.{}({args})", decl.sig.fn_ident);
    }

    let _ = annotated;
    let args = render_op_args(decl, inputs, bindings);
    format!("fut::{}({args})", decl.sig.fn_ident)
}

fn render_op_args(decl: &OpDecl, inputs: &BTreeMap<String, Value>, bindings: &[crate::scan::SetupBinding]) -> String {
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
        let v = inputs.get(p);
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

/// Emit a `serde_yaml::from_str::<Type>(r#"..."#).unwrap()` expression.
fn yaml_deser(v: &Value, ty: &str) -> String {
    let yaml_str = serde_yaml::to_string(v).unwrap_or_else(|_| "~\n".to_string());
    format!("serde_yaml::from_str::<{ty}>({yaml_str:?}).unwrap()")
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
            fixture_pkg_root: None,
            is_local: false,
        };

        let fixture_src = workspace_root.join("crates/specgate-harness/src/lib.rs");
        let result = generate(scratch.path(), &fixture_src, &config);
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
            fixture_pkg_root: None,
            is_local: true,
        };

        let fixture_src = workspace_root.join("crates/specgate-harness/src/lib.rs");
        let result = generate(scratch.path(), &fixture_src, &config);
        assert!(result.is_ok(), "generate failed: {:?}", result.err());

        let manifest = std::fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("path ="),
            "manifest should contain path deps when is_local=true, got:\n{manifest}"
        );
    }
}
