use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_yaml::Value;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpecDocument {
    pub name: String,
    #[serde(default)]
    pub binding: Option<BindingDecl>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub state: BTreeMap<String, String>,
    #[serde(default)]
    pub init: BTreeMap<String, Value>,
    #[serde(default)]
    pub operations: BTreeMap<String, Value>,
    #[serde(default)]
    pub invariants: BTreeMap<String, String>,
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

impl SpecDocument {
    #[must_use]
    pub fn binding_name(&self) -> Option<String> {
        self.binding
            .as_ref()
            .and_then(BindingDecl::first)
            .map(|binding| binding.name.clone())
    }

    #[must_use]
    pub fn target(&self) -> Option<String> {
        self.binding
            .as_ref()
            .and_then(BindingDecl::first)
            .map(|binding| binding.target.clone())
    }
}

impl<'de> Deserialize<'de> for SpecDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSpecDocument {
            name: String,
            #[serde(default)]
            binding: Option<RawBindingDecl>,
            #[serde(default)]
            target: Option<String>,
            #[serde(default)]
            depends_on: Vec<String>,
            #[serde(default)]
            state: BTreeMap<String, String>,
            #[serde(default)]
            init: BTreeMap<String, Value>,
            #[serde(default)]
            operations: BTreeMap<String, Value>,
            #[serde(default)]
            invariants: BTreeMap<String, String>,
            #[serde(default)]
            inputs: BTreeMap<String, Value>,
            #[serde(default)]
            types: BTreeMap<String, Value>,
            outcome: Value,
            #[serde(default)]
            outputs: BTreeMap<String, Value>,
            #[serde(default)]
            cases: Vec<SpecCase>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawBindingDecl {
            Legacy(String),
            Single(BindingEntry),
            Multiple(Vec<BindingEntry>),
        }

        let raw = RawSpecDocument::deserialize(deserializer)?;
        let binding = match raw.binding {
            Some(RawBindingDecl::Legacy(name)) => {
                let target = raw
                    .target
                    .ok_or_else(|| de::Error::missing_field("target"))?;
                Some(BindingDecl::Single(BindingEntry { name, target }))
            }
            Some(RawBindingDecl::Single(binding)) => Some(BindingDecl::Single(binding)),
            Some(RawBindingDecl::Multiple(bindings)) => Some(BindingDecl::Multiple(bindings)),
            None => None,
        };

        Ok(Self {
            name: raw.name,
            binding,
            depends_on: raw.depends_on,
            state: raw.state,
            init: raw.init,
            operations: raw.operations,
            invariants: raw.invariants,
            inputs: raw.inputs,
            types: raw.types,
            outcome: raw.outcome,
            outputs: raw.outputs,
            cases: raw.cases,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BindingEntry {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum BindingDecl {
    Single(BindingEntry),
    Multiple(Vec<BindingEntry>),
}

impl BindingDecl {
    #[must_use]
    pub fn first(&self) -> Option<&BindingEntry> {
        match self {
            Self::Single(binding) => Some(binding),
            Self::Multiple(bindings) => bindings.first(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecCase {
    pub name: String,
    pub desc: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub expected: BTreeMap<String, Value>,
    #[serde(default)]
    pub steps: Vec<TestStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TestStep {
    pub operation: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub expected: BTreeMap<String, Value>,
    #[serde(default)]
    pub assert_state: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::{BindingDecl, BindingEntry, SpecDocument};

    #[test]
    fn deserializes_legacy_binding_format() {
        let spec: SpecDocument = serde_yaml::from_str(
            "name: legacy\nbinding: rust\ntarget: test\noutcome: Ok\ncases: []\n",
        )
        .expect("legacy spec should deserialize");

        assert_eq!(
            spec.binding,
            Some(BindingDecl::Single(BindingEntry {
                name: "rust".to_string(),
                target: "test".to_string(),
            }))
        );
        assert_eq!(spec.binding_name().as_deref(), Some("rust"));
        assert_eq!(spec.target().as_deref(), Some("test"));
    }

    #[test]
    fn deserializes_new_binding_format() {
        let spec: SpecDocument = serde_yaml::from_str(
            "name: modern\nbinding:\n  - name: rust\n    target: test\noutcome: Ok\ncases: []\n",
        )
        .expect("new spec should deserialize");

        assert_eq!(spec.binding_name().as_deref(), Some("rust"));
        assert_eq!(spec.target().as_deref(), Some("test"));
    }
}
