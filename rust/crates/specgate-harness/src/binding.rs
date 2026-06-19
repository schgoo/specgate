//! Binding YAML resolution.

use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Target {
    pub package_root: PathBuf,
    pub command: Option<String>,
}

#[derive(Debug)]
pub struct Binding {
    pub language: String,
    pub targets: BTreeMap<String, Target>,
}

impl Binding {
    /// Get a target by name, or the target named "default" (falling back to
    /// the first target) if name is None.
    pub fn target(&self, name: Option<&str>) -> Option<&Target> {
        match name {
            Some(n) => self.targets.get(n),
            None => self
                .targets
                .get("default")
                .or_else(|| self.targets.values().next()),
        }
    }

    /// Get the package_root for a target (convenience for backward compat).
    pub fn package_root(&self, target_name: Option<&str>) -> Option<&Path> {
        self.target(target_name).map(|t| t.package_root.as_path())
    }
}

pub fn load_binding(path: &Path) -> Option<Binding> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_yaml::from_str(&text).ok()?;
    let map = v.as_mapping()?;
    let language = map
        .get(Value::String("language".into()))?
        .as_str()?
        .to_string();
    let targets_map = map.get(Value::String("targets".into()))?.as_mapping()?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();

    let mut targets = BTreeMap::new();
    for (k, v) in targets_map {
        let name = k.as_str()?;
        let target_map = v.as_mapping()?;
        let pkg = target_map
            .get(Value::String("package_root".into()))
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let command = target_map
            .get(Value::String("command".into()))
            .and_then(|v| v.as_str())
            .map(String::from);
        targets.insert(
            name.to_string(),
            Target {
                package_root: normalize(&dir.join(pkg)),
                command,
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
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
