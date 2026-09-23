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

    fn focused_binding(language: &str) -> PathBuf {
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join(format!("{language}.binding.yaml"))
    }

    #[test]
    fn focused_discovery_batches_each_language_and_preserves_parity() {
        use specgate_discovery::discovery::discover_many_target;

        let components = ["fixture.cli.multiple", "fixture.cli.replay", "fixture.cli.setup"];
        let rust = discover_many_target(focused_binding("rust").to_str().unwrap(), None, &components).expect("batched Rust discovery");
        assert_eq!(rust.raw_registry_json.len(), 1, "Rust link-time discovery self-reports once");
        assert_eq!(rust.registries.len(), 1);
        for component in components {
            let discovered = rust
                .components
                .get(component)
                .unwrap_or_else(|| panic!("missing Rust metadata for {component}"));
            assert_eq!(discovered.registry_index, 0, "every Rust component shares one document");
            assert!(discovered.schema.is_ok(), "{component} normalization failed");
        }
        assert!(rust.registry("fixture.cli.replay").is_some());
        assert!(rust.raw_registry_json("fixture.cli.setup").is_some());

        let replay = rust.schema("fixture.cli.replay").unwrap().as_ref().unwrap();
        assert_eq!(replay.operations.len(), 2);
        assert!(replay.types.is_empty());
        let setup = rust.schema("fixture.cli.setup").unwrap().as_ref().unwrap();
        assert_eq!(setup.operations.len(), 1);
        assert_eq!(setup.types.len(), 1);
        assert_eq!(setup.operations[0].name, "increment");
        assert_eq!(setup.operations[0].inputs.len(), 1);
        assert_eq!(setup.operations[0].inputs[0].name, "initial");
        let multiple = rust.schema("fixture.cli.multiple").unwrap().as_ref().unwrap();
        assert_eq!(
            multiple
                .operations
                .iter()
                .map(|operation| operation.name.as_str())
                .collect::<Vec<_>>(),
            vec!["unexercised", "used"]
        );

        let csharp = discover_many_target(focused_binding("csharp").to_str().unwrap(), None, &components).expect("batched C# discovery");
        assert_eq!(
            csharp.raw_registry_json.len(),
            components.len(),
            "C# reflection emits one document per component"
        );
        for (position, component) in components.iter().enumerate() {
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

        // Rich structured/sum-type discovery remains exhaustively covered by
        // the complete 69-row CTSC golden matrix. This fixture isolates CLI
        // batching, setup folding, source operation identity, and C# parity.
        let schema_json = serde_json::to_string(replay).unwrap();
        let first = encode_schema_registry_result(
            "urn:ctsc:registry:fixture.cli.replay".to_string(),
            "1.0.0".to_string(),
            &schema_json,
        )
        .unwrap()
        .registry_json;
        let second = encode_schema_registry_result(
            "urn:ctsc:registry:fixture.cli.replay".to_string(),
            "1.0.0".to_string(),
            &schema_json,
        )
        .unwrap()
        .registry_json;
        assert_eq!(first, second);
        assert!(!first.contains('\n'), "registry JSON must be compact");
        let document: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(document["format"], "ctsc.registry");
        assert_eq!(document["formatVersion"], "0.2.0");
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
