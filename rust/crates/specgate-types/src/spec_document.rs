use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecDocument {
    pub name: String,
    #[serde(default)]
    pub binding: Option<String>,
    pub target: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub types: BTreeMap<String, Value>,
    pub outcome: Value,
    #[serde(default)]
    pub outputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub cases: Vec<SpecCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecCase {
    pub name: String,
    pub desc: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub expected: BTreeMap<String, Value>,
}
