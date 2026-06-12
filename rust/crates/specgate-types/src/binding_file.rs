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
    pub kind: BindingTargetKind,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BindingTargetKind {
    Command,
    Api,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingTargetOutputs {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub stdout: Option<String>,
}
