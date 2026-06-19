//! Generate a temporary Cargo project that compiles + executes a fixture
//! against the spec's cases and writes a JSON trace to disk.

use crate::scan::{AnnotatedSource, OpDecl};
use crate::spec::{Case, Setup, Spec};
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct GeneratedProject {
    pub crate_dir: PathBuf,
    pub trace_file: PathBuf,
}

pub fn generate(
    scratch_dir: &Path,
    fixture_src: &Path,
    spec: &Spec,
    cases_to_run: &[&Case],
    annotated: &AnnotatedSource,
    workspace_root: &Path,
    needs_async: bool,
) -> std::io::Result<GeneratedProject> {
    std::fs::create_dir_all(scratch_dir.join("src"))?;
    let trace_file = scratch_dir.join("traces.json");

    let annotations_path = workspace_root.join("crates/specgate-annotations");
    let runtime_path = workspace_root.join("crates/specgate-runtime");
    let macros_path = workspace_root.join("crates/specgate-annotations-macros");
    let harness_path = workspace_root.join("crates/specgate-harness");

    let manifest = format!(
        r#"[package]
name = "sg-runner"
version = "0.0.1"
edition = "2024"

[[bin]]
name = "sg-runner"
path = "src/main.rs"

[dependencies]
specgate-annotations = {{ path = "{ann}" }}
specgate-harness = {{ path = "{harness}" }}

[workspace]

[patch.crates-io]
proc-macro2 = {{ git = "https://github.com/dtolnay/proc-macro2", tag = "1.0.106" }}
quote = {{ git = "https://github.com/dtolnay/quote", tag = "1.0.45" }}
unicode-ident = {{ git = "https://github.com/dtolnay/unicode-ident", tag = "1.0.20" }}
syn = {{ git = "https://github.com/dtolnay/syn", tag = "2.0.118" }}
serde = {{ git = "https://github.com/serde-rs/serde", tag = "v1.0.228" }}
serde_core = {{ git = "https://github.com/serde-rs/serde", tag = "v1.0.228" }}
serde_derive = {{ git = "https://github.com/serde-rs/serde", tag = "v1.0.228" }}
serde_yaml = {{ git = "https://github.com/dtolnay/serde-yaml", tag = "0.9.34" }}
serde_json = {{ git = "https://github.com/serde-rs/json", tag = "v1.0.145" }}
itoa = {{ git = "https://github.com/dtolnay/itoa", tag = "1.0.18" }}
ryu = {{ git = "https://github.com/dtolnay/ryu", tag = "1.0.21" }}
unsafe-libyaml = {{ git = "https://github.com/dtolnay/unsafe-libyaml", tag = "0.2.11" }}
indexmap = {{ git = "https://github.com/indexmap-rs/indexmap", tag = "2.14.0" }}
equivalent = {{ git = "https://github.com/indexmap-rs/equivalent", tag = "v1.0.2" }}
hashbrown = {{ git = "https://github.com/rust-lang/hashbrown", tag = "v0.17.1" }}
"#,
        ann = annotations_path.display().to_string().replace('\\', "/"),
        harness = harness_path.display().to_string().replace('\\', "/"),
    );
    let _ = runtime_path;
    let _ = macros_path;
    std::fs::write(scratch_dir.join("Cargo.toml"), manifest)?;

    // Seed the tmp project's Cargo.lock from the parent workspace so cargo
    // doesn't need to consult crates.io (the env may have it blocked).
    let parent_lock = workspace_root.join("Cargo.lock");
    let tmp_lock = scratch_dir.join("Cargo.lock");
    if parent_lock.exists() {
        let _ = std::fs::copy(&parent_lock, &tmp_lock);
    }

    let main_rs = render_main(fixture_src, spec, cases_to_run, annotated, &trace_file, needs_async)?;
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
) -> std::io::Result<String> {
    let mut out = String::new();
    let abs = std::fs::canonicalize(fixture_src)?;
    out.push_str("#![allow(unused, unused_mut, unused_variables, dead_code, clippy::all)]\n");
    out.push_str("use specgate_annotations::{TraceEvent, take_traces, reset, set_mock, SpecEvent};\n");
    out.push_str(&format!(
        "#[path = \"{}\"] mod fut;\n",
        abs.display().to_string().replace('\\', "\\\\")
    ));
    out.push_str("use fut::*;\n");
    out.push_str("\n");
    out.push_str("fn panic_msg(e: &Box<dyn std::any::Any + Send>) -> String {\n");
    out.push_str("    if let Some(s) = e.downcast_ref::<String>() { return s.clone(); }\n");
    out.push_str("    if let Some(s) = e.downcast_ref::<&'static str>() { return s.to_string(); }\n");
    out.push_str("    \"panic\".to_string()\n");
    out.push_str("}\n\n");

    if needs_async {
        out.push_str(ASYNC_BLOCK_ON);
    }

    out.push_str("fn main() {\n");
    out.push_str(
        "    let out_path = std::env::args().nth(1).expect(\"missing output path\");\n",
    );
    out.push_str("    let mut all: std::collections::BTreeMap<String, Vec<TraceEvent>> = std::collections::BTreeMap::new();\n");

    for case in cases_to_run {
        out.push_str(&format!("    // ---- case: {} ----\n", case.name));
        out.push_str("    {\n");
        out.push_str("        reset();\n");
        render_case(&mut out, case, spec, annotated);
        out.push_str(&format!(
            "        all.insert({:?}.to_string(), take_traces());\n",
            case.name
        ));
        out.push_str("    }\n");
    }

    out.push_str(&format!(
        "    let s = serde_json_lite_to_string(&all);\n    std::fs::write({:?}, s).expect(\"write traces\");\n",
        trace_out.display().to_string()
    ));
    out.push_str("}\n\n");

    // Inline a tiny JSON serializer to avoid pulling serde_json into the
    // generated crate. We only need to emit our own TraceEvent shape.
    out.push_str(JSON_HELPER);

    Ok(out)
}

/// A minimal no-op-waker block_on. Sufficient for fixture async fns that
/// don't yield to a real reactor — they complete on the first poll.
const ASYNC_BLOCK_ON: &str = r#"
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
"#;


const JSON_HELPER: &str = r#"
fn serde_json_lite_to_string(map: &std::collections::BTreeMap<String, Vec<TraceEvent>>) -> String {
    fn esc(s: &str) -> String {
        let mut o = String::with_capacity(s.len() + 2);
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
        o
    }
    let mut s = String::from("{");
    let mut first = true;
    for (k, v) in map.iter() {
        if !first { s.push(','); }
        first = false;
        s.push_str(&esc(k));
        s.push(':');
        s.push('[');
        let mut f2 = true;
        for ev in v {
            if !f2 { s.push(','); }
            f2 = false;
            match ev {
                TraceEvent::Event { name, value } => {
                    s.push_str("{\"kind\":\"Event\",\"name\":");
                    s.push_str(&esc(name));
                    s.push_str(",\"value\":");
                    s.push_str(&esc(value));
                    s.push('}');
                }
                TraceEvent::Run { operation } => {
                    s.push_str("{\"kind\":\"Run\",\"operation\":");
                    s.push_str(&esc(operation));
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
            out.push_str(&format!("        set_mock({:?}, &[\n", k));
            for (mk, mv) in m {
                if let (Some(ks), Some(vs)) = (mk.as_str(), mv.as_str()) {
                    out.push_str(&format!("            ({:?}, {:?}),\n", ks, vs));
                }
            }
            out.push_str("        ]);\n");
        }
    }

    // Setups: bind to variables.
    let mut setup_vars: Vec<(String, String)> = Vec::new(); // (var_name, setup_fn_name)
    match &case.setup {
        Setup::None => {}
        Setup::Single(name) => {
            let sig = annotated.setups.get(name);
            let args = render_setup_args(sig, &case.inputs);
            let var = sanitize_ident(name);
            out.push_str(&format!(
                "        let mut {var} = fut::{name}({args});\n"
            ));
            if let Some(sig) = sig {
                if annotated.spec_event_structs.contains(sig.return_type.trim()) {
                    out.push_str(&format!(
                        "        SpecEvent::emit_fields(&{var}, None);\n"
                    ));
                }
            }
            setup_vars.push((var, name.clone()));
        }
        Setup::Multi(entries) => {
            for (alias, fn_name) in entries {
                let sig = annotated.setups.get(fn_name);
                let args = render_setup_args(sig, &case.inputs);
                let var = sanitize_ident(alias);
                out.push_str(&format!(
                    "        let mut {var} = fut::{fn_name}({args});\n"
                ));
                if let Some(sig) = sig {
                    if annotated.spec_event_structs.contains(sig.return_type.trim()) {
                        out.push_str(&format!(
                            "        SpecEvent::emit_fields(&{var}, Some({:?}));\n",
                            alias
                        ));
                    }
                }
                setup_vars.push((var, fn_name.clone()));
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
        let mut call = render_op_call(op, decl, &case.inputs, &setup_vars, annotated);
        if spec.async_ops.contains(op) {
            call = format!("sg_block_on({call})");
        }
        out.push_str("        {\n");
        out.push_str(&format!(
            "            let __r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{ {call}; }}));\n"
        ));
        out.push_str("            if let Err(__e) = __r {\n");
        out.push_str(&format!(
            "                let msg = panic_msg(&__e);\n                specgate_annotations::emit_event(\"{op}.outcome\", \"Unrecoverable\");\n                specgate_annotations::emit_event(\"{op}.error\", &msg);\n"
        ));
        out.push_str("            }\n");
        out.push_str("        }\n");
    }
}

fn render_setup_args(
    sig: Option<&crate::scan::FnSig>,
    inputs: &BTreeMap<String, Value>,
) -> String {
    let Some(sig) = sig else { return String::new() };
    let mut parts = Vec::new();
    for (p, ty) in &sig.params {
        let v = inputs.get(p);
        parts.push(value_to_rust(v, ty));
    }
    parts.join(", ")
}

fn render_op_call(
    op_name: &str,
    decl: Option<&OpDecl>,
    inputs: &BTreeMap<String, Value>,
    setup_vars: &[(String, String)],
    annotated: &AnnotatedSource,
) -> String {
    let Some(decl) = decl else {
        return format!("fut::{op_name}()");
    };

    // Method: pick the matching receiver variable.
    if decl.takes_self {
        let recv = setup_vars
            .iter()
            .find(|(_, fn_name)| {
                annotated
                    .setups
                    .get(fn_name)
                    .map(|s| decl.method_of.as_deref() == Some(s.return_type.trim()))
                    .unwrap_or(false)
            })
            .or_else(|| setup_vars.first())
            .cloned();
        let recv_var = recv
            .map(|(v, _)| v)
            .unwrap_or_else(|| "/* missing receiver */".to_string());
        let args = render_op_args(decl, inputs, setup_vars);
        return format!("{recv_var}.{}({args})", decl.sig.fn_ident);
    }

    let args = render_op_args(decl, inputs, setup_vars);
    format!("fut::{}({args})", decl.sig.fn_ident)
}

fn render_op_args(
    decl: &OpDecl,
    inputs: &BTreeMap<String, Value>,
    setup_vars: &[(String, String)],
) -> String {
    let mut parts = Vec::new();
    for (p, ty) in &decl.sig.params {
        // If the param name matches a setup alias, pass that variable.
        if let Some((var, _)) = setup_vars.iter().find(|(v, _)| v == p) {
            let prefix = if ty.starts_with("&mut") {
                "&mut "
            } else if ty.starts_with('&') {
                "&"
            } else {
                ""
            };
            parts.push(format!("{prefix}{var}"));
            continue;
        }
        let v = inputs.get(p);
        parts.push(value_to_rust(v, ty));
    }
    parts.join(", ")
}

fn sanitize_ident(s: &str) -> String {
    s.replace(['-', '.', ' '], "_")
}

fn value_to_rust(v: Option<&Value>, ty: &str) -> String {
    let ty = ty.trim();
    let Some(v) = v else {
        return "Default::default()".into();
    };
    let ty_norm = ty.trim_start_matches('&').trim_start_matches("mut ").trim();
    match v {
        Value::Number(n) => {
            // Suffix int with type.
            if ty_norm.starts_with('i') || ty_norm.starts_with('u') {
                format!("{}{}", n, ty_norm)
            } else if ty_norm == "f32" || ty_norm == "f64" {
                format!("{}{}", n, ty_norm)
            } else {
                n.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::String(s) => {
            // For &str / String / etc.
            if ty_norm == "String" {
                format!("{:?}.to_string()", s)
            } else {
                format!("{:?}", s)
            }
        }
        Value::Sequence(seq) => {
            // For &[i32] or Vec<i32> etc., pick element type by stripping outer.
            let elem_ty = inner_ty(ty_norm);
            let elements: Vec<String> = seq
                .iter()
                .map(|e| value_to_rust(Some(e), elem_ty.as_deref().unwrap_or("i32")))
                .collect();
            format!("&[{}][..]", elements.join(", "))
        }
        Value::Null => "Default::default()".into(),
        Value::Mapping(_) => "Default::default()".into(),
        Value::Tagged(t) => value_to_rust(Some(&t.value), ty),
    }
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
