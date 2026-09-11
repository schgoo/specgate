//! `specgate discover <binding.yaml> ...` — discover one implementation target
//! and encode its raw metadata as a deterministic CTSC registry document.

use std::path::Path;

use specgate::{SpecEvent, spec_operation};
use specgate_ctsc::encode_discovery_registry_result;

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
    let raw_json = match specgate_harness::discover_registry_json(binding, target_name, component) {
        Ok(json) => json,
        Err(reason) => return DiscoverOutcome::Error { reason },
    };
    let encoded = match encode_discovery_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        component.to_string(),
        &raw_json,
    ) {
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

    fn discover_stateless(binding: &Path, output: &Path) -> DiscoverOutcome {
        discover(
            binding.to_str().expect("utf-8 binding path"),
            "",
            "fixture.stateless_add",
            "urn:ctsc:registry:fixture.stateless-add:1",
            "1.0.0",
            output.to_str().expect("utf-8 output path"),
        )
    }

    #[test]
    fn discover_rust_stateless_registry() {
        let output = output_path("discover-rust");
        let binding = repo_root()
            .join("test")
            .join("rust")
            .join("crates")
            .join("specgate-fixtures")
            .join("specs")
            .join("binding.yaml");

        let outcome = discover_stateless(&binding, &output);

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
        let root = repo_root();
        let rust_output = output_path("discover-parity-rust");
        let csharp_output = output_path("discover-parity-csharp");
        let rust_binding = root
            .join("test")
            .join("rust")
            .join("crates")
            .join("specgate-fixtures")
            .join("specs")
            .join("binding.yaml");
        let csharp_binding = root
            .join("test")
            .join("rust")
            .join("crates")
            .join("specgate-fixtures")
            .join("specs")
            .join("csharp.yaml");

        let rust = discover_stateless(&rust_binding, &rust_output);
        let csharp = discover_stateless(&csharp_binding, &csharp_output);

        assert!(matches!(rust, DiscoverOutcome::Complete { .. }), "Rust discovery failed: {rust}");
        assert!(matches!(csharp, DiscoverOutcome::Complete { .. }), "C# discovery failed: {csharp}");
        assert_eq!(
            std::fs::read(&rust_output).expect("read Rust registry"),
            std::fs::read(&csharp_output).expect("read C# registry"),
            "Rust and C# registry JSON must be byte-identical"
        );
        let _ = std::fs::remove_file(rust_output);
        let _ = std::fs::remove_file(csharp_output);
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
