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
    let schema = match specgate_discovery::discovery::discover_target_schema(binding, target_name, component) {
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

    fn repo_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|path| path.join("rust").join("Cargo.toml").is_file())
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
        repo_root().join("test").join("bindings").join("rust.yaml")
    }

    fn csharp_binding() -> PathBuf {
        repo_root().join("test").join("bindings").join("csharp.yaml")
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
        let document = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        assert_eq!(document["format"], "ctsc.registry");
        assert_eq!(document["formatVersion"], "0.2.0");
        assert!(!json.contains('\n'), "registry JSON must be compact");
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn discover_csharp_stateless_registry() {
        assert_registry_parity("fixture.stateless_add", "urn:ctsc:registry:fixture.stateless-add:1", "stateless");
    }

    #[test]
    fn discover_rust_complex_registry() {
        let output = output_path("discover-complex-rust");
        let outcome = discover_component(&rust_binding(), &output, "fixture.rich", "urn:ctsc:registry:fixture.rich:1");
        let report = assert_complete(&outcome, "Rust");
        assert_eq!(report.operations, 2);
        assert_eq!(report.types, 3);

        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&output).expect("read complex registry")).expect("valid registry JSON");
        let component = &document["components"][0];
        assert_eq!(component["types"][0]["name"], "Address");
        assert_eq!(component["types"][2]["name"], "Shape");
        assert_eq!(
            component["operations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|operation| operation["name"] == "describe")
                .unwrap()["outcomes"]["result"],
            serde_json::json!({
                "kind": "primitive",
                "name": "string"
            })
        );
        assert_eq!(component["operations"][0]["outcomes"]["empty"], true);
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn discover_csharp_complex_registry() {
        assert_registry_parity("fixture.rich", "urn:ctsc:registry:fixture.rich:1", "complex");
    }

    #[test]
    fn discover_rust_setup_folded_registry() {
        let output = output_path("discover-setup-rust");
        let outcome = discover_component(&rust_binding(), &output, "fixture.setup", "urn:ctsc:registry:fixture.setup:1");
        let report = assert_complete(&outcome, "Rust");
        assert_eq!(report.operations, 2);
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
    fn discover_csharp_setup_folded_registry() {
        assert_registry_parity("fixture.setup", "urn:ctsc:registry:fixture.setup:1", "setup");
    }

    #[test]
    fn discover_csharp_unscoped_setup_and_fallible_unit_parity() {
        assert_registry_parity(
            "fixture.fallible_unit",
            "urn:ctsc:registry:fixture.fallible-unit:1",
            "fallible-unit",
        );

        let discovered =
            specgate_discovery::discovery::discover_target(csharp_binding().to_str().unwrap(), None, "fixture.fallible_unit").unwrap();
        let setup = discovered.registry.setups_for("fixture.fallible_unit", "advance");
        assert_eq!(setup.len(), 1, "unscoped C# setup metadata must be retained");
        let void = discovered
            .schema
            .operations
            .iter()
            .find(|operation| operation.name == "fallible_void")
            .unwrap();
        assert!(void.output.is_empty());
        assert!(!void.is_async);
        assert_eq!(void.errors[0].ty, "string");
        let task = discovered
            .schema
            .operations
            .iter()
            .find(|operation| operation.name == "fallible_task")
            .unwrap();
        assert!(task.output.is_empty());
        assert!(task.is_async);
        assert_eq!(task.errors[0].ty, "string");
    }

    #[test]
    fn discover_many_shares_one_pass_and_keeps_cross_language_schemas() {
        use specgate_discovery::discovery::discover_many_target;

        let rust_components = ["fixture.rich", "fixture.setup", "fixture.stateless_add"];
        let rust = discover_many_target(rust_binding().to_str().unwrap(), None, &rust_components).expect("batched Rust discovery");
        assert_eq!(rust.raw_registry_json.len(), 1, "Rust link-time discovery self-reports once");
        assert_eq!(rust.registries.len(), 1);
        for component in rust_components {
            let discovered = rust
                .components
                .get(component)
                .unwrap_or_else(|| panic!("missing Rust metadata for {component}"));
            assert_eq!(discovered.registry_index, 0, "every Rust component shares one document");
            assert!(discovered.schema.is_ok(), "{component} normalization failed");
        }
        assert!(rust.registry("fixture.rich").is_some());
        assert!(rust.raw_registry_json("fixture.setup").is_some());

        let csharp_components = ["fixture.rich", "fixture.stateless_add"];
        let csharp = discover_many_target(csharp_binding().to_str().unwrap(), None, &csharp_components).expect("batched C# discovery");
        assert_eq!(csharp.raw_registry_json.len(), 2, "C# reflection emits one document per component");
        for (position, component) in csharp_components.iter().enumerate() {
            let discovered = csharp
                .components
                .get(*component)
                .unwrap_or_else(|| panic!("missing C# metadata for {component}"));
            assert_eq!(discovered.registry_index, position, "C# documents stay aligned with request order");
            let csharp_schema = discovered.schema.as_ref().expect("C# schema");
            let rust_schema = rust.schema(component).expect("Rust schema").as_ref().expect("Rust schema");
            assert_eq!(
                csharp_schema, rust_schema,
                "batched C# discovery must normalize to the Rust canonical for {component}"
            );
        }
    }

    #[test]
    fn discover_returns_error_without_panicking() {
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
