//! Binding YAML resolution.

use serde_yaml::Value;
use std::path::{Path, PathBuf};

#[derive(Debug)]
#[allow(dead_code)]
pub struct Binding {
    pub language: String,
    pub package_root: PathBuf,
}

pub fn load_binding(path: &Path) -> Option<Binding> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_yaml::from_str(&text).ok()?;
    let map = v.as_mapping()?;
    let language = map
        .get(Value::String("language".into()))?
        .as_str()?
        .to_string();
    let targets = map.get(Value::String("targets".into()))?.as_mapping()?;
    let target = targets.values().next()?.as_mapping()?;
    let pkg = target
        .get(Value::String("package_root".into()))?
        .as_str()?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let package_root = normalize(&dir.join(pkg));
    Some(Binding {
        language,
        package_root,
    })
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
