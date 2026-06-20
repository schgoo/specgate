use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BindingFile {
    pub language: String,
    #[serde(default)]
    pub targets: BTreeMap<String, BindingTarget>,
}

impl<'de> Deserialize<'de> for BindingFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawBindingTarget {
            #[serde(default)]
            package_root: Option<String>,
            #[serde(default)]
            test_root: Option<String>,
            #[serde(default)]
            build: Option<String>,
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            function: Option<String>,
            #[serde(default)]
            constructor: Option<String>,
            #[serde(default)]
            outputs: BindingTargetOutputs,
        }

        #[derive(Deserialize)]
        struct RawBindingFile {
            #[serde(default)]
            language: Option<String>,
            #[serde(default)]
            targets: Option<BTreeMap<String, RawBindingTarget>>,
        }

        let raw = RawBindingFile::deserialize(deserializer)?;
        let language = raw.language.ok_or_else(|| de::Error::custom("missing required field 'language'"))?;
        let mut targets = BTreeMap::new();

        for (name, target) in raw.targets.unwrap_or_default() {
            let package_root = target
                .package_root
                .ok_or_else(|| de::Error::custom("missing required field 'package_root'"))?;

            if target.command.is_some() && target.function.is_some() {
                return Err(de::Error::custom("target cannot have both command and function"));
            }

            targets.insert(
                name,
                BindingTarget {
                    package_root,
                    test_root: target.test_root,
                    build: target.build,
                    command: target.command,
                    function: target.function,
                    constructor: target.constructor,
                    outputs: target.outputs,
                },
            );
        }

        Ok(Self { language, targets })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingTarget {
    pub package_root: String,
    #[serde(default)]
    pub test_root: Option<String>,
    #[serde(default)]
    pub build: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub constructor: Option<String>,
    #[serde(default)]
    pub outputs: BindingTargetOutputs,
}

impl BindingTarget {
    #[must_use]
    pub fn is_command(&self) -> bool {
        self.command.is_some()
    }

    #[must_use]
    pub fn is_api(&self) -> bool {
        self.function.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingTargetOutputs {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub stdout: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::BindingFile;

    #[test]
    fn deserializes_target_package_root_and_kind_helpers() {
        let binding: BindingFile = serde_yaml::from_str(
            r#"
language: rust
targets:
  test:
    package_root: ../rust/crates/my-app
    test_root: ../rust/crates/my-app/tests
    command: "cargo test -p my-app"
  generate:
    package_root: ../rust/crates/my-backend
    function: "backend::generate"
"#,
        )
        .expect("binding should deserialize");

        assert_eq!(binding.targets["test"].package_root, "../rust/crates/my-app");
        assert_eq!(binding.targets["test"].test_root.as_deref(), Some("../rust/crates/my-app/tests"));
        assert!(binding.targets["test"].is_command());
        assert!(!binding.targets["test"].is_api());
        assert_eq!(binding.targets["generate"].package_root, "../rust/crates/my-backend");
        assert!(binding.targets["generate"].is_api());
        assert!(!binding.targets["generate"].is_command());
    }

    #[test]
    fn rejects_target_without_package_root() {
        let error = serde_yaml::from_str::<BindingFile>(
            r#"
language: rust
targets:
  test:
    command: "cargo test"
"#,
        )
        .expect_err("binding without package_root should fail");

        assert_eq!(error.to_string(), "missing required field 'package_root'");
    }
}
