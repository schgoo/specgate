//! `specgate capture <binding.yaml> --out <dir>` — capture passing Rust tests
//! as one deterministic native CTSC reference bundle.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specgate::__rt::{NativeCapture, NativeCaptureConfig, NativeCaptureEnvironmentConfig};
use specgate::{SpecEvent, spec_operation};
use specgate_ctsc::{encode_native_captures_otlp_result, encode_schema_registry_result};
use specgate_harness::discovery::{Registry, cargo_bin};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const REGISTRY_FILE: &str = "registry.ctsc.json";
const TRACE_FILE: &str = "reference.otlp.json";
const MANIFEST_FILE: &str = "manifest.json";
const REGISTRY_VERSION: &str = "0.1.0";

/// Summary of a capture run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct CaptureReport {
    #[spec_event]
    pub component_id: String,
    #[spec_event]
    pub scenarios: i32,
    #[spec_event]
    pub operations: i32,
    #[spec_event]
    pub registry_path: String,
    #[spec_event]
    pub trace_path: String,
    #[spec_event]
    pub manifest_path: String,
}

/// Outcome of `capture`.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum CaptureOutcome {
    Complete { report: CaptureReport },
    Error { reason: String },
}

impl std::fmt::Display for CaptureOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureOutcome::Complete { report } => write!(
                f,
                "Complete(component={}, scenarios={}, operations={}, registry={}, trace={}, manifest={})",
                report.component_id, report.scenarios, report.operations, report.registry_path, report.trace_path, report.manifest_path
            ),
            CaptureOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BindingFile {
    language: String,
    targets: BTreeMap<String, BindingTarget>,
}

#[derive(Debug, Deserialize)]
struct BindingTarget {
    #[serde(default = "default_package_root")]
    package_root: String,
}

fn default_package_root() -> String {
    ".".to_string()
}

#[derive(Debug)]
struct ResolvedTarget {
    name: String,
    language: String,
    package_root: PathBuf,
}

#[derive(Debug)]
struct TestBinary {
    label: String,
    executable: PathBuf,
    is_library: bool,
}

#[derive(Debug)]
struct IsolatedTest {
    scenario_name: String,
    test_name: String,
    executable: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureManifest {
    format: &'static str,
    format_version: &'static str,
    component_id: String,
    target: ManifestTarget,
    tool: ManifestTool,
    registry: ManifestRegistry,
    reference: ManifestReference,
    scenarios: ManifestScenarios,
}

#[derive(Debug, Serialize)]
struct ManifestTarget {
    name: String,
    language: String,
}

#[derive(Debug, Serialize)]
struct ManifestTool {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ManifestRegistry {
    path: &'static str,
    id: String,
    version: &'static str,
    digest: String,
}

#[derive(Debug, Serialize)]
struct ManifestReference {
    path: &'static str,
    digest: String,
}

#[derive(Debug, Serialize)]
struct ManifestScenarios {
    count: i32,
    names: Vec<String>,
}

/// Capture passing tests for one Rust binding target as a native CTSC bundle.
///
/// Empty `target` selects the binding default. Empty `component` selects the
/// sole discovered component and errors when discovery is ambiguous.
#[must_use]
#[spec_operation("capture")]
pub fn capture(binding: &str, target: &str, component: &str, out: &str) -> CaptureOutcome {
    match capture_result(binding, target, component, out) {
        Ok(report) => CaptureOutcome::Complete { report },
        Err(reason) => CaptureOutcome::Error { reason },
    }
}

fn capture_result(binding: &str, target: &str, component: &str, out: &str) -> Result<CaptureReport, String> {
    if out.is_empty() {
        return Err("capture requires a non-empty output directory".to_string());
    }
    let resolved = resolve_target(binding, target)?;
    if resolved.language != "rust" {
        return Err(format!(
            "capture currently supports only Rust targets; binding language is '{}'",
            resolved.language
        ));
    }
    if !resolved.package_root.join("Cargo.toml").is_file() {
        return Err(format!(
            "capture target '{}' is not a Rust package (no Cargo.toml at {})",
            resolved.name,
            resolved.package_root.display()
        ));
    }

    let target_name = if target.is_empty() { None } else { Some(target) };
    let discovery_json = specgate_harness::discover_registry_json(binding, target_name, "")?;
    let registry = Registry::parse(&discovery_json)?;
    let selected = select_component(&registry, component)?;
    let schema = specgate_harness::discover_target_schema(binding, target_name, &selected)?;
    let schema_json =
        serde_json::to_string(&schema).map_err(|error| format!("failed to serialize normalized discovery schema: {error}"))?;
    let registry_id = format!("urn:ctsc:registry:{selected}");
    let registry_encoding = encode_schema_registry_result(registry_id.clone(), REGISTRY_VERSION.to_string(), &schema_json)?;
    let registry_bytes = registry_encoding.registry_json.into_bytes();
    let registry_digest = sha256_digest(&registry_bytes);

    let scratch = capture_scratch_dir(&resolved.package_root)?;
    let test_binaries = build_test_binaries(&resolved.package_root, &scratch)?;
    let tests = enumerate_tests(&test_binaries)?;
    let captures = capture_passing_tests(&tests, &selected, &scratch)?;
    if captures.is_empty() {
        return Err(format!(
            "no passing tests captured operations for component '{selected}'; add a handwritten test that invokes the component"
        ));
    }

    let operation_count = captures.iter().try_fold(0_usize, |count, capture| {
        count
            .checked_add(capture.operations.len())
            .ok_or_else(|| "captured operation count overflow".to_string())
    })?;
    let trace_encoding = encode_native_captures_otlp_result(
        &captures,
        env!("CARGO_PKG_VERSION"),
        &resolved.name,
        &resolved.language,
        &registry_id,
        REGISTRY_VERSION,
        &registry_digest,
    )?;
    let trace_bytes = trace_encoding.otlp_json.into_bytes();
    let trace_digest = sha256_digest(&trace_bytes);
    let scenarios = i32::try_from(captures.len()).map_err(|_error| "captured scenario count exceeds i32".to_string())?;
    let operations = i32::try_from(operation_count).map_err(|_error| "captured operation count exceeds i32".to_string())?;
    let manifest = CaptureManifest {
        format: "specgate.capture-manifest",
        format_version: "0.1.0",
        component_id: selected.clone(),
        target: ManifestTarget {
            name: resolved.name,
            language: resolved.language,
        },
        tool: ManifestTool {
            name: "specgate",
            version: env!("CARGO_PKG_VERSION"),
        },
        registry: ManifestRegistry {
            path: REGISTRY_FILE,
            id: registry_id,
            version: REGISTRY_VERSION,
            digest: registry_digest,
        },
        reference: ManifestReference {
            path: TRACE_FILE,
            digest: trace_digest,
        },
        scenarios: ManifestScenarios {
            count: scenarios,
            names: captures.iter().map(|capture| capture.scenario_name.clone()).collect(),
        },
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|error| format!("failed to serialize capture manifest: {error}"))?;

    let output_dir = Path::new(out);
    std::fs::create_dir_all(output_dir)
        .map_err(|error| format!("failed to create capture output directory {}: {error}", output_dir.display()))?;
    write_artifact(&output_dir.join(REGISTRY_FILE), &registry_bytes)?;
    write_artifact(&output_dir.join(TRACE_FILE), &trace_bytes)?;
    write_artifact(&output_dir.join(MANIFEST_FILE), &manifest_bytes)?;

    Ok(CaptureReport {
        component_id: selected,
        scenarios,
        operations,
        registry_path: report_artifact_path(out, REGISTRY_FILE),
        trace_path: report_artifact_path(out, TRACE_FILE),
        manifest_path: report_artifact_path(out, MANIFEST_FILE),
    })
}

fn resolve_target(binding_path: &str, requested_target: &str) -> Result<ResolvedTarget, String> {
    let binding_path = PathBuf::from(binding_path);
    let text = std::fs::read_to_string(&binding_path).map_err(|error| {
        format!(
            "binding '{binding_path_display}' not found or invalid: {error}",
            binding_path_display = binding_path.display()
        )
    })?;
    let binding: BindingFile =
        serde_yaml::from_str(&text).map_err(|error| format!("binding '{}' not found or invalid: {error}", binding_path.display()))?;
    let (name, target) = if requested_target.is_empty() {
        binding
            .targets
            .get_key_value("default")
            .or_else(|| binding.targets.first_key_value())
            .ok_or_else(|| format!("binding '{}' contains no targets", binding_path.display()))?
    } else {
        binding.targets.get_key_value(requested_target).ok_or_else(|| {
            format!(
                "target '{requested_target}' not found in binding; available targets: {}",
                binding.targets.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?
    };
    let base = binding_path.parent().unwrap_or_else(|| Path::new("."));
    let package_root = std::fs::canonicalize(base.join(&target.package_root)).unwrap_or_else(|_| base.join(&target.package_root));
    Ok(ResolvedTarget {
        name: name.clone(),
        language: binding.language,
        package_root,
    })
}

fn select_component(registry: &Registry, component: &str) -> Result<String, String> {
    let components = registry.present_components();
    if component.is_empty() {
        return match components.len() {
            0 => Err("no components found: target has no annotated operations or types".to_string()),
            1 => Ok(components[0].clone()),
            _ => Err(format!(
                "multiple components present ({}); select one with --component <id>",
                components.join(", ")
            )),
        };
    }
    if components.iter().any(|candidate| candidate == component) {
        Ok(component.to_string())
    } else {
        Err(format!(
            "component '{component}' not found; available components: {}",
            components.join(", ")
        ))
    }
}

fn capture_scratch_dir(package_root: &Path) -> Result<PathBuf, String> {
    let package_name = package_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("could not derive package name from {}", package_root.display()))?;
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("Rust workspace root")
        .join("target")
        .join("specgate-capture")
        .join(package_name);
    std::fs::create_dir_all(&scratch)
        .map_err(|error| format!("failed to create capture scratch directory {}: {error}", scratch.display()))?;
    Ok(scratch)
}

fn build_test_binaries(package_root: &Path, scratch: &Path) -> Result<Vec<TestBinary>, String> {
    let mut command = Command::new(cargo_bin());
    command.arg("test").arg("--no-run").arg("--quiet").arg("--message-format=json");
    command.current_dir(package_root);
    command.env_remove("RUSTC_WORKSPACE_WRAPPER");
    command.env_remove("CARGO");
    command.env_remove("CARGO_MANIFEST_DIR");
    command.env("CARGO_TARGET_DIR", scratch.join("target"));

    let output = command
        .output()
        .map_err(|error| format!("failed to build capture test binaries: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "capture test build failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut binaries = Vec::new();
    let mut seen = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact")
            || !message
                .get("profile")
                .and_then(|profile| profile.get("test"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let Some(executable) = message.get("executable").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !seen.insert(executable.to_string()) {
            continue;
        }
        let target = &message["target"];
        let label = target["name"].as_str().unwrap_or("test").to_string();
        let is_library = target["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("lib")));
        binaries.push(TestBinary {
            label,
            executable: PathBuf::from(executable),
            is_library,
        });
    }
    binaries.sort_by(|left, right| {
        right
            .is_library
            .cmp(&left.is_library)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.executable.cmp(&right.executable))
    });
    if binaries.is_empty() {
        return Err("capture test build produced no libtest binaries".to_string());
    }
    Ok(binaries)
}

fn enumerate_tests(binaries: &[TestBinary]) -> Result<Vec<IsolatedTest>, String> {
    let mut tests = Vec::new();
    for binary in binaries {
        let output = Command::new(&binary.executable)
            .arg("--list")
            .arg("--format")
            .arg("terse")
            .output()
            .map_err(|error| format!("failed to list tests in {}: {error}", binary.executable.display()))?;
        if !output.status.success() {
            return Err(format!(
                "capture test enumeration failed for {}: {}",
                binary.executable.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some(test_name) = line.strip_suffix(": test") else {
                continue;
            };
            let test_name = test_name.trim().to_string();
            let scenario_name = if binary.is_library {
                test_name.clone()
            } else {
                format!("{}::{test_name}", binary.label)
            };
            tests.push(IsolatedTest {
                scenario_name,
                test_name,
                executable: binary.executable.clone(),
            });
        }
    }
    tests.sort_by(|left, right| left.scenario_name.cmp(&right.scenario_name));
    Ok(tests)
}

fn capture_passing_tests(tests: &[IsolatedTest], selected_component: &str, scratch: &Path) -> Result<Vec<NativeCapture>, String> {
    let sidecars = scratch.join("sidecars");
    std::fs::create_dir_all(&sidecars)
        .map_err(|error| format!("failed to create capture sidecar directory {}: {error}", sidecars.display()))?;
    let mut captures = Vec::new();
    for (index, test) in tests.iter().enumerate() {
        let sidecar = sidecars.join(format!("{index:08}.json"));
        let _ = std::fs::remove_file(&sidecar);
        let environment = NativeCaptureEnvironmentConfig {
            capture: NativeCaptureConfig {
                scenario_name: test.scenario_name.clone(),
                trace_id: "11111111111111111111111111111111".to_string(),
                run_span_id: "1111111111111101".to_string(),
                scenario_span_id: "1111111111111102".to_string(),
                operation_span_ids: Vec::new(),
                start_time_unix_nano: 0,
                clock_step_unix_nano: 1,
            },
            sidecar_path: sidecar.clone(),
        };
        let environment_json =
            serde_json::to_string(&environment).map_err(|error| format!("failed to serialize native capture environment: {error}"))?;
        let output = Command::new(&test.executable)
            .arg(&test.test_name)
            .arg("--exact")
            .arg("--test-threads=1")
            .env("SPECGATE_NATIVE_CAPTURE", environment_json)
            .output()
            .map_err(|error| format!("failed to run isolated test '{}': {error}", test.scenario_name))?;
        if !output.status.success() {
            let _ = std::fs::remove_file(&sidecar);
            continue;
        }
        if !sidecar.is_file() {
            continue;
        }
        let bytes =
            std::fs::read(&sidecar).map_err(|error| format!("failed to read native capture sidecar {}: {error}", sidecar.display()))?;
        let _ = std::fs::remove_file(&sidecar);
        let capture: NativeCapture = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse native capture sidecar for '{}': {error}", test.scenario_name))?;
        let selected_count = capture
            .operations
            .iter()
            .filter(|operation| operation.component_id == selected_component)
            .count();
        if selected_count == 0 {
            continue;
        }
        if let Some(foreign) = capture
            .operations
            .iter()
            .find(|operation| operation.component_id != selected_component)
        {
            return Err(format!(
                "captured scenario '{}' invokes foreign component '{}' operation '{}'; capture cannot link it against root registry '{}'",
                test.scenario_name, foreign.component_id, foreign.operation_name, selected_component
            ));
        }
        captures.push(capture);
    }
    Ok(captures)
}

fn write_artifact(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("failed to write capture artifact {}: {error}", path.display()))
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn report_artifact_path(out: &str, filename: &str) -> String {
    let separator = if out.contains('\\') && !out.contains('/') {
        '\\'
    } else if out.contains('/') {
        '/'
    } else {
        std::path::MAIN_SEPARATOR
    };
    let trimmed = out.trim_end_matches(['/', '\\']);
    format!("{trimmed}{separator}{filename}")
}

/// Format a capture outcome for CLI display.
#[must_use]
pub fn format_outcome(outcome: &CaptureOutcome) -> String {
    format!("{outcome}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Output;
    use std::sync::Mutex;

    static CAPTURE_LOCK: Mutex<()> = Mutex::new(());

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_path_buf()
    }

    fn rust_binding() -> PathBuf {
        repo_root()
            .join("test")
            .join("rust")
            .join("crates")
            .join("specgate-fixtures")
            .join("specs")
            .join("binding.yaml")
    }

    fn output_dir(label: &str) -> PathBuf {
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-capture-test-{label}-{}", std::process::id()))
    }

    #[test]
    fn capture_stateless_bundle_is_linked_and_byte_identical() {
        let _guard = CAPTURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let first_dir = output_dir("first");
        let second_dir = output_dir("second");
        let _ = std::fs::remove_dir_all(&first_dir);
        let _ = std::fs::remove_dir_all(&second_dir);

        let first = capture(
            rust_binding().to_str().unwrap(),
            "",
            "fixture.stateless_add",
            first_dir.to_str().unwrap(),
        );
        let second = capture(
            rust_binding().to_str().unwrap(),
            "",
            "fixture.stateless_add",
            second_dir.to_str().unwrap(),
        );
        let CaptureOutcome::Complete { report } = first else {
            panic!("first capture failed: {first}");
        };
        assert!(matches!(second, CaptureOutcome::Complete { .. }), "second capture failed: {second}");
        assert_eq!(report.component_id, "fixture.stateless_add");
        assert_eq!(report.scenarios, 1);
        assert_eq!(report.operations, 1);

        let first_names = directory_file_names(&first_dir);
        assert_eq!(first_names, vec![MANIFEST_FILE, TRACE_FILE, REGISTRY_FILE]);
        for filename in [REGISTRY_FILE, TRACE_FILE, MANIFEST_FILE] {
            assert_eq!(
                std::fs::read(first_dir.join(filename)).unwrap(),
                std::fs::read(second_dir.join(filename)).unwrap(),
                "{filename} must be byte-identical across repeated capture"
            );
        }

        let registry = std::fs::read(first_dir.join(REGISTRY_FILE)).unwrap();
        let trace = std::fs::read(first_dir.join(TRACE_FILE)).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(first_dir.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(manifest["format"], "specgate.capture-manifest");
        assert_eq!(manifest["formatVersion"], "0.1.0");
        assert_eq!(manifest["componentId"], "fixture.stateless_add");
        assert_eq!(manifest["target"], serde_json::json!({"name":"default","language":"rust"}));
        assert_eq!(
            manifest["tool"],
            serde_json::json!({"name":"specgate","version":env!("CARGO_PKG_VERSION")})
        );
        assert_eq!(manifest["registry"]["path"], REGISTRY_FILE);
        assert_eq!(manifest["registry"]["id"], "urn:ctsc:registry:fixture.stateless_add");
        assert_eq!(manifest["registry"]["version"], REGISTRY_VERSION);
        assert_eq!(manifest["registry"]["digest"], sha256_digest(&registry));
        assert_eq!(manifest["reference"]["path"], TRACE_FILE);
        assert_eq!(manifest["reference"]["digest"], sha256_digest(&trace));
        assert_eq!(manifest["scenarios"]["count"], 1);
        assert_eq!(
            manifest["scenarios"]["names"],
            serde_json::json!(["conformance::basic::stateless_add::adds_two_and_three"])
        );
        validate_bundle_with_python(&first_dir);

        let _ = std::fs::remove_dir_all(first_dir);
        let _ = std::fs::remove_dir_all(second_dir);
    }

    #[test]
    fn capture_errors_are_actionable() {
        let _guard = CAPTURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let ambiguous = capture(rust_binding().to_str().unwrap(), "", "", output_dir("ambiguous").to_str().unwrap());
        assert!(matches!(
            ambiguous,
            CaptureOutcome::Error { reason } if reason.contains("multiple components present") && reason.contains("--component")
        ));

        let csharp_binding = repo_root()
            .join("test")
            .join("rust")
            .join("crates")
            .join("specgate-fixtures")
            .join("specs")
            .join("csharp.yaml");
        let unsupported = capture(
            csharp_binding.to_str().unwrap(),
            "",
            "fixture.stateless_add",
            output_dir("unsupported").to_str().unwrap(),
        );
        assert!(matches!(
            unsupported,
            CaptureOutcome::Error { reason } if reason.contains("only Rust targets") && reason.contains("csharp")
        ));
        assert!(matches!(
            capture(rust_binding().to_str().unwrap(), "", "fixture.stateless_add", ""),
            CaptureOutcome::Error { reason } if reason.contains("non-empty output directory")
        ));
    }

    #[test]
    fn capture_errors_when_no_passing_test_invokes_component() {
        let _guard = CAPTURE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let out = output_dir("no-scenarios");
        let _ = std::fs::remove_dir_all(&out);
        let outcome = capture(rust_binding().to_str().unwrap(), "", "fixture.named_inputs", out.to_str().unwrap());
        assert!(matches!(
            outcome,
            CaptureOutcome::Error { reason } if reason.contains("no passing tests captured operations")
        ));
        assert!(!out.exists());
    }

    fn directory_file_names(path: &Path) -> Vec<&str> {
        let mut names = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .map(|name| name.to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
            .into_iter()
            .map(|name| match name.as_str() {
                MANIFEST_FILE => MANIFEST_FILE,
                REGISTRY_FILE => REGISTRY_FILE,
                TRACE_FILE => TRACE_FILE,
                other => panic!("unexpected capture artifact: {other}"),
            })
            .collect()
    }

    fn validate_bundle_with_python(output_dir: &Path) {
        let validator = repo_root().join("docs").join("ctsc").join("validate.py");
        if !validator.is_file() {
            eprintln!("CTSC validator unavailable: {}", validator.display());
            return;
        }
        for (kind, args) in [
            ("registry", vec![output_dir.join(REGISTRY_FILE)]),
            ("trace", vec![output_dir.join(TRACE_FILE)]),
            ("linked", vec![output_dir.join(TRACE_FILE), output_dir.join(REGISTRY_FILE)]),
        ] {
            let Some(output) = invoke_python(&validator, kind, &args) else {
                eprintln!("CTSC validator unavailable: Python was not found");
                return;
            };
            assert_validator_output(&output, kind);
        }
    }

    fn invoke_python(validator: &Path, kind: &str, documents: &[PathBuf]) -> Option<Output> {
        for (program, prefix_args) in [("python", &[][..]), ("py", &["-3"][..])] {
            let output = Command::new(program)
                .args(prefix_args)
                .arg(validator)
                .arg(kind)
                .args(documents)
                .output();
            match output {
                Ok(output) if python_launcher_is_unavailable(&output) => {}
                Ok(output) => return Some(output),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("failed to invoke CTSC validator with {program}: {error}"),
            }
        }
        None
    }

    fn python_launcher_is_unavailable(output: &Output) -> bool {
        !output.status.success()
            && (String::from_utf8_lossy(&output.stdout).contains("Python was not found")
                || String::from_utf8_lossy(&output.stderr).contains("Python was not found"))
    }

    fn assert_validator_output(output: &Output, kind: &str) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success()
            && (stdout.contains("missing validator dependencies") || stderr.contains("missing validator dependencies"))
        {
            eprintln!("CTSC validator unavailable: {stdout}{stderr}");
            return;
        }
        assert!(
            output.status.success(),
            "CTSC {kind} validator rejected generated bundle:\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}
