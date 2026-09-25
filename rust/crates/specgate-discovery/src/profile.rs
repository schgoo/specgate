//! Language-neutral capture profile parsing and exact registry resolution.

use crate::discovery::Registry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One exact CTSC operation identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSelector {
    pub component: String,
    pub operation: String,
}

/// Capture profile v1.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureProfile {
    version: u32,
    include: Vec<OperationSelector>,
}

/// Read and strictly validate a capture profile.
///
/// # Errors
///
/// Returns an actionable error for unreadable or malformed YAML, unsupported
/// versions, empty values, an empty selector list, or duplicate selectors.
pub fn read_capture_profile(path: &Path) -> Result<CaptureProfile, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to read capture profile {}: {error}", path.display()))?;
    let profile: CaptureProfile =
        serde_yaml::from_slice(&bytes).map_err(|error| format!("invalid capture profile {}: {error}", path.display()))?;
    profile.validate()
}

impl CaptureProfile {
    fn validate(self) -> Result<Self, String> {
        if self.version != 1 {
            return Err(format!("unsupported capture profile version {}; expected 1", self.version));
        }
        if self.include.is_empty() {
            return Err("capture profile include must contain at least one selector".to_string());
        }
        let mut unique = BTreeSet::new();
        for selector in &self.include {
            if selector.component.trim().is_empty() || selector.operation.trim().is_empty() {
                return Err("capture profile selector component and operation must not be empty".to_string());
            }
            if !unique.insert(selector) {
                return Err(format!(
                    "capture profile contains duplicate selector '{}::{}'",
                    selector.component, selector.operation
                ));
            }
        }
        Ok(self)
    }

    /// Resolve this profile against one or more compiled registry documents.
    ///
    /// The result is sorted by component then operation and is independent of
    /// source language or registry document partitioning.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing component or operation.
    pub fn resolve(&self, registries: &[&Registry]) -> Result<Vec<OperationSelector>, String> {
        let mut available = BTreeMap::<String, BTreeSet<String>>::new();
        for registry in registries {
            for operation in registry.ops.iter().filter(|operation| !operation.is_setup) {
                available
                    .entry(operation.component.clone())
                    .or_default()
                    .insert(operation.name.clone());
            }
        }
        for selector in &self.include {
            let Some(operations) = available.get(&selector.component) else {
                return Err(format!(
                    "capture profile component '{}' not found; available components: {}",
                    selector.component,
                    available.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            };
            if !operations.contains(&selector.operation) {
                return Err(format!(
                    "capture profile operation '{}::{}' not found; available operations: {}",
                    selector.component,
                    selector.operation,
                    operations.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        let mut resolved = self.include.clone();
        resolved.sort();
        Ok(resolved)
    }
}

/// Expand one component shorthand to all of its exact operation identities.
///
/// # Errors
///
/// Returns an error when the component is absent.
pub fn resolve_component(registry: &Registry, component: &str) -> Result<Vec<OperationSelector>, String> {
    let mut operations = registry
        .ops
        .iter()
        .filter(|operation| !operation.is_setup && operation.component == component)
        .map(|operation| OperationSelector {
            component: component.to_string(),
            operation: operation.name.clone(),
        })
        .collect::<Vec<_>>();
    operations.sort();
    operations.dedup();
    if operations.is_empty() {
        return Err(format!("component '{component}' has no operations"));
    }
    Ok(operations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(component: &str, operations: &[&str]) -> Registry {
        let operations = operations
            .iter()
            .map(|operation| {
                serde_json::json!({
                    "name": operation, "module_path": "fixture", "fn_name": operation,
                    "is_setup": false, "is_async": false, "is_method": false,
                    "is_public": true, "return_type": "()", "fills": "",
                    "params": [], "component": component
                })
            })
            .collect::<Vec<_>>();
        Registry::parse(&serde_json::json!({"operations": operations, "types": []}).to_string()).unwrap()
    }

    #[test]
    fn resolver_is_sorted_and_registry_partition_neutral() {
        let profile: CaptureProfile = serde_yaml::from_str(
            "version: 1\ninclude:\n  - component: fixture.b\n    operation: z\n  - component: fixture.a\n    operation: a\n",
        )
        .unwrap();
        let profile = profile.validate().unwrap();
        let rust = Registry::parse(
            &serde_json::json!({
                "operations": [
                    serde_json::json!({"name":"z","module_path":"f","fn_name":"z","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.b"}),
                    serde_json::json!({"name":"a","module_path":"f","fn_name":"a","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.a"})
                ],
                "types": []
            })
            .to_string(),
        )
        .unwrap();
        let csharp_a = registry("fixture.a", &["a"]);
        let csharp_b = registry("fixture.b", &["z"]);
        assert_eq!(
            profile.resolve(&[&rust]).unwrap(),
            profile.resolve(&[&csharp_b, &csharp_a]).unwrap()
        );
        assert_eq!(profile.resolve(&[&rust]).unwrap()[0].component, "fixture.a");
    }

    #[test]
    fn rejects_profile_contract_and_preflight_failures() {
        for yaml in [
            "version: 2\ninclude:\n  - component: a\n    operation: b\n",
            "version: 1\ninclude: []\n",
            "version: 1\nunknown: true\ninclude:\n  - component: a\n    operation: b\n",
            "version: 1\ninclude:\n  - component: a\n    operation: b\n    unknown: true\n",
            "version: 1\ninclude:\n  - component: ''\n    operation: b\n",
            "version: 1\ninclude:\n  - component: '   '\n    operation: b\n",
            "version: 1\ninclude:\n  - component: a\n    operation: b\n  - component: a\n    operation: b\n",
        ] {
            let parsed = serde_yaml::from_str::<CaptureProfile>(yaml)
                .map_err(|error| error.to_string())
                .and_then(CaptureProfile::validate);
            assert!(parsed.is_err(), "{yaml}");
        }
        let profile = serde_yaml::from_str::<CaptureProfile>("version: 1\ninclude:\n  - component: missing\n    operation: b\n")
            .unwrap()
            .validate()
            .unwrap();
        assert!(
            profile
                .resolve(&[&registry("a", &["b"])])
                .unwrap_err()
                .contains("component 'missing' not found")
        );
    }
}
