//! `specgate discover <binding.yaml> ...` — discover one implementation target
//! and encode its normalized, setup-folded schema as a deterministic CTSC
//! registry document.

use std::path::Path;

use specgate::{SpecEvent, spec_operation};
use specgate_ctsc::encode_schema_registry_result;

/// Summary of a discovery run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct DiscoverReport {
    #[spec_event]
    pub component_id: String,
    #[spec_event]
    pub operations: i32,
    #[spec_event]
    pub types: i32,
    #[spec_event]
    pub output_path: String,
}

/// Outcome of `discover`.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum DiscoverOutcome {
    Complete { report: DiscoverReport },
    Error { reason: String },
}

impl std::fmt::Display for DiscoverOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoverOutcome::Complete { report } => write!(
                f,
                "Complete(component={}, operations={}, types={}, output={})",
                report.component_id, report.operations, report.types, report.output_path
            ),
            DiscoverOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

/// Discover one binding target and write its component metadata as compact
/// CTSC registry JSON.
///
/// An empty `target` selects the binding's default target. Output parent
/// directories are created when needed.
#[must_use]
#[spec_operation("discover")]
pub fn discover(binding: &str, target: &str, component: &str, registry_id: &str, registry_version: &str, out: &str) -> DiscoverOutcome {
    let target_name = if target.is_empty() { None } else { Some(target) };
    let schema = match specgate_harness::discover_target_schema(binding, target_name, component) {
        Ok(schema) => schema,
        Err(reason) => return DiscoverOutcome::Error { reason },
    };
    let schema_json = match serde_json::to_string(&schema) {
        Ok(json) => json,
        Err(error) => {
            return DiscoverOutcome::Error {
                reason: format!("failed to serialize normalized discovery schema: {error}"),
            };
        }
    };
    let encoded = match encode_schema_registry_result(registry_id.to_string(), registry_version.to_string(), &schema_json) {
        Ok(encoded) => encoded,
        Err(reason) => return DiscoverOutcome::Error { reason },
    };

    let out_path = Path::new(out);
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return DiscoverOutcome::Error {
            reason: format!("failed to create output directory {}: {error}", parent.display()),
        };
    }
    if let Err(error) = std::fs::write(out_path, encoded.registry_json) {
        return DiscoverOutcome::Error {
            reason: format!("failed to write registry to {out}: {error}"),
        };
    }

    DiscoverOutcome::Complete {
        report: DiscoverReport {
            component_id: component.to_string(),
            operations: encoded.operation_count,
            types: encoded.type_count,
            output_path: out.to_string(),
        },
    }
}

/// Format a discovery outcome for CLI display.
#[must_use]
pub fn format_outcome(outcome: &DiscoverOutcome) -> String {
    format!("{outcome}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    static DISCOVERY_LOCK: Mutex<()> = Mutex::new(());

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_path_buf()
    }

    fn output_path(label: &str) -> PathBuf {
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-cli-{label}-{}.json", std::process::id()))
    }

    fn discover_component(binding: &Path, output: &Path, component: &str, registry_id: &str) -> DiscoverOutcome {
        discover(
            binding.to_str().expect("utf-8 binding path"),
            "",
            component,
            registry_id,
            "1.0.0",
            output.to_str().expect("utf-8 output path"),
        )
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

    fn csharp_binding() -> PathBuf {
        repo_root()
            .join("test")
            .join("rust")
            .join("crates")
            .join("specgate-fixtures")
            .join("specs")
            .join("csharp.yaml")
    }

    fn assert_complete<'a>(outcome: &'a DiscoverOutcome, language: &str) -> &'a DiscoverReport {
        let DiscoverOutcome::Complete { report } = outcome else {
            panic!("{language} discovery failed: {outcome}");
        };
        report
    }

    fn assert_registry_parity(component: &str, registry_id: &str, label: &str) {
        let rust_output = output_path(&format!("{label}-parity-rust"));
        let csharp_output = output_path(&format!("{label}-parity-csharp"));
        let rust = discover_component(&rust_binding(), &rust_output, component, registry_id);
        let csharp = discover_component(&csharp_binding(), &csharp_output, component, registry_id);

        assert_complete(&rust, "Rust");
        assert_complete(&csharp, "C#");
        assert_eq!(
            std::fs::read(&rust_output).expect("read Rust registry"),
            std::fs::read(&csharp_output).expect("read C# registry"),
            "Rust and C# registry JSON must be byte-identical"
        );
        let _ = std::fs::remove_file(rust_output);
        let _ = std::fs::remove_file(csharp_output);
    }

    #[test]
    fn discover_rust_stateless_registry() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = output_path("discover-rust");
        let outcome = discover_component(
            &rust_binding(),
            &output,
            "fixture.stateless_add",
            "urn:ctsc:registry:fixture.stateless-add:1",
        );

        let DiscoverOutcome::Complete { report } = outcome else {
            panic!("Rust discovery failed: {outcome}");
        };
        assert_eq!(report.component_id, "fixture.stateless_add");
        assert_eq!(report.operations, 1);
        assert_eq!(report.types, 0);
        let json = std::fs::read_to_string(&output).expect("read registry output");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["format"], "ctsc.registry");
        assert!(!json.contains('\n'), "registry JSON must be compact");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    #[ignore = "builds C# via dotnet; slow"]
    fn discover_csharp_stateless_registry() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_registry_parity("fixture.stateless_add", "urn:ctsc:registry:fixture.stateless-add:1", "stateless");
    }

    #[test]
    fn discover_rust_complex_registry() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = output_path("discover-complex-rust");
        let outcome = discover_component(
            &rust_binding(),
            &output,
            "fixture.complex_inputs",
            "urn:ctsc:registry:fixture.complex-inputs:1",
        );
        let report = assert_complete(&outcome, "Rust");
        assert_eq!(report.operations, 13);
        assert_eq!(report.types, 6);

        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).expect("read complex registry")).expect("valid registry JSON");
        let component = &document["components"][0];
        assert_eq!(component["types"][0]["name"], "Address");
        assert_eq!(component["types"][5]["name"], "Shape");
        assert_eq!(
            component["operations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|operation| operation["name"] == "find_point")
                .unwrap()["outcomes"]["result"],
            serde_json::json!({
                "kind": "tagged_union",
                "variants": [
                    {"name": "None"},
                    {"name": "Some", "payload": {"kind": "named", "name": "Point"}}
                ]
            })
        );
        let _ = std::fs::remove_file(output);
    }

    #[test]
    #[ignore = "builds C# via dotnet; slow"]
    fn discover_csharp_complex_registry() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_registry_parity("fixture.complex_inputs", "urn:ctsc:registry:fixture.complex-inputs:1", "complex");
    }

    #[test]
    fn discover_rust_setup_folded_registry() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = output_path("discover-setup-rust");
        let outcome = discover_component(
            &rust_binding(),
            &output,
            "fixture.setup_with_params",
            "urn:ctsc:registry:fixture.setup-with-params:1",
        );
        let report = assert_complete(&outcome, "Rust");
        assert_eq!(report.operations, 1);
        assert_eq!(report.types, 1);

        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).expect("read setup registry")).expect("valid registry JSON");
        assert_eq!(
            document["components"][0]["operations"][0]["inputs"],
            serde_json::json!([
                {"name": "initial", "type": {"kind": "primitive", "name": "i32"}}
            ]),
            "setup construction input must be folded without receiver/Counter inputs"
        );
        let _ = std::fs::remove_file(output);
    }

    #[test]
    #[ignore = "builds C# via dotnet; slow"]
    fn discover_csharp_setup_folded_registry() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_registry_parity(
            "fixture.setup_with_params",
            "urn:ctsc:registry:fixture.setup-with-params:1",
            "setup",
        );
    }

    #[test]
    fn discover_returns_error_without_panicking() {
        let _guard = DISCOVERY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = output_path("discover-error");
        let outcome = discover(
            "missing-binding.yaml",
            "",
            "fixture.stateless_add",
            "urn:ctsc:registry:fixture.stateless-add:1",
            "1.0.0",
            output.to_str().expect("utf-8 output path"),
        );
        assert!(matches!(outcome, DiscoverOutcome::Error { .. }));
        assert!(!output.exists());
    }
}
