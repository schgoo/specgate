//! Binding YAML resolution.

use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Async runtime the generated runner uses to drive async operations to
/// completion (Rust targets only). Defaults to [`Runtime::Smol`] when a target
/// declares no `runtime:` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Runtime {
    /// `smol::block_on` — real executor + on-demand reactor (the default).
    #[default]
    Smol,
    /// A current-thread tokio runtime (`tokio::runtime::Builder`).
    Tokio,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub package_root: PathBuf,
    pub command: Option<String>,
    pub runtime: Runtime,
    pub framework: Option<String>,
}

#[derive(Debug)]
pub struct Binding {
    #[allow(dead_code)]
    pub language: String,
    pub targets: BTreeMap<String, Target>,
}

impl Binding {
    /// Get a target by name, or the target named "default" (falling back to
    /// the first target) if name is None.
    pub fn target(&self, name: Option<&str>) -> Option<&Target> {
        match name {
            Some(n) => self.targets.get(n),
            None => self.targets.get("default").or_else(|| self.targets.values().next()),
        }
    }

    /// Get the `package_root` for a target (convenience for backward compat).
    #[allow(dead_code)]
    pub fn package_root(&self, target_name: Option<&str>) -> Option<&Path> {
        self.target(target_name).map(|t| t.package_root.as_path())
    }
}

pub fn load_binding(path: &Path) -> Option<Binding> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_yaml::from_str(&text).ok()?;
    let map = v.as_mapping()?;
    let language = map.get(Value::String("language".into()))?.as_str()?.to_string();
    let targets_map = map.get(Value::String("targets".into()))?.as_mapping()?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();

    let mut targets = BTreeMap::new();
    for (k, v) in targets_map {
        let name = k.as_str()?;
        let entry_map = v.as_mapping()?;
        let pkg = entry_map
            .get(Value::String("package_root".into()))
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let command = entry_map
            .get(Value::String("command".into()))
            .and_then(|v| v.as_str())
            .map(String::from);
        let runtime = match entry_map.get(Value::String("runtime".into())).and_then(|v| v.as_str()) {
            Some("tokio") => Runtime::Tokio,
            // Unknown / absent → default smol. (Schema validation constrains the
            // accepted values; the parser is permissive.)
            _ => Runtime::Smol,
        };
        let framework = entry_map
            .get(Value::String("framework".into()))
            .and_then(|v| v.as_str())
            .map(String::from);
        targets.insert(
            name.to_string(),
            Target {
                package_root: normalize(&dir.join(pkg)),
                command,
                runtime,
                framework,
            },
        );
    }

    Some(Binding { language, targets })
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> Option<Binding> {
        let tmp = tempfile_yaml(yaml);
        load_binding(Path::new(&tmp))
    }

    /// Write YAML content to a NamedTempFile-equivalent inside the test's
    /// scratch space. Because we cannot use /tmp, we write to a fixed path
    /// under the cargo target directory.
    fn tempfile_yaml(content: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("specgate-harness-unit-tests")
            .join("binding_test_scratch");
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join(format!("binding_test_{id}.yaml"));
        std::fs::write(&path, content).expect("write temp yaml");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn parses_framework_field() {
        let b = parse("language: csharp\ntargets:\n  default:\n    package_root: .\n    framework: net8.0\n").expect("binding");
        let t = b.target(None).expect("default target");
        assert_eq!(t.framework.as_deref(), Some("net8.0"));
    }

    #[test]
    fn framework_absent_is_none() {
        let b = parse("language: csharp\ntargets:\n  default:\n    package_root: .\n").expect("binding");
        let t = b.target(None).expect("default target");
        assert!(t.framework.is_none());
    }

    #[test]
    fn parses_multiple_targets_framework_independent() {
        let yaml = "language: csharp\ntargets:\n  v8:\n    package_root: .\n    framework: net8.0\n  v10:\n    package_root: .\n";
        let b = parse(yaml).expect("binding");
        assert_eq!(b.target(Some("v8")).unwrap().framework.as_deref(), Some("net8.0"));
        assert!(b.target(Some("v10")).unwrap().framework.is_none());
    }
}
