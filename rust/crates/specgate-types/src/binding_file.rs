use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingFile {
    pub language: String,
    pub project_root: String,
    #[serde(default)]
    pub targets: BTreeMap<String, BindingTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingTarget {
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
