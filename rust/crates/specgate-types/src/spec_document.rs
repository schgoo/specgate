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
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            binding: Option<RawBindingDecl>,
            #[serde(default)]
            target: Option<String>,
            #[serde(default)]
            depends_on: Option<Vec<String>>,
            #[serde(default)]
            state: Option<BTreeMap<String, String>>,
            #[serde(default)]
            init: Option<BTreeMap<String, Value>>,
            #[serde(default)]
            operations: Option<BTreeMap<String, Value>>,
            #[serde(default)]
            invariants: Option<BTreeMap<String, String>>,
            #[serde(default)]
            inputs: Option<BTreeMap<String, Value>>,
            #[serde(default)]
            types: Option<BTreeMap<String, Value>>,
            #[serde(default)]
            outcome: Option<Value>,
            #[serde(default)]
            outputs: Option<BTreeMap<String, Value>>,
            #[serde(default)]
            cases: Option<Vec<SpecCase>>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawBindingDecl {
            Legacy(String),
            Single(BindingEntry),
            Multiple(Vec<BindingEntry>),
        }

        let raw = RawSpecDocument::deserialize(deserializer)?;
        let name = raw
            .name
            .ok_or_else(|| de::Error::custom("missing required field: name"))?;
        let cases = raw
            .cases
            .ok_or_else(|| de::Error::custom("missing required field: cases"))?;

        if raw.inputs.is_some() && raw.operations.is_some() {
            return Err(de::Error::custom(
                "inputs and operations are mutually exclusive",
            ));
        }

        if raw.state.is_some() && raw.init.is_none() {
            return Err(de::Error::custom("state requires init"));
        }

        for case in &cases {
            if !case.inputs.is_empty() && !case.steps.is_empty() {
                return Err(de::Error::custom("case cannot have both inputs and steps"));
            }
        }

        if raw.operations.is_some() {
            for case in &cases {
                if case.steps.is_empty() && (!case.inputs.is_empty() || !case.expected.is_empty()) {
                    return Err(de::Error::custom("state machine cases must use steps"));
                }
            }
        } else if cases.iter().any(|case| !case.steps.is_empty()) {
            return Err(de::Error::custom("steps require operations section"));
        }

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
            name,
            binding,
            depends_on: raw.depends_on.unwrap_or_default(),
            state: raw.state.unwrap_or_default(),
            init: raw.init.unwrap_or_default(),
            operations: raw.operations.unwrap_or_default(),
            invariants: raw.invariants.unwrap_or_default(),
            inputs: raw.inputs.unwrap_or_default(),
            types: raw.types.unwrap_or_default(),
            outcome: raw.outcome.unwrap_or(Value::Null),
            outputs: raw.outputs.unwrap_or_default(),
            cases,
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

    #[test]
    fn rejects_state_machine_case_with_expected_but_no_steps() {
        let error = serde_yaml::from_str::<SpecDocument>(
            r#"
name: test.machine
state:
  count: int
init:
  count: 0
operations:
  increment:
    inputs: { amount: int }
cases:
  - name: bad
    desc: Missing steps
    expected:
      outcome: Ok
"#,
        )
        .expect_err("state machine case without steps should fail");

        assert_eq!(error.to_string(), "state machine cases must use steps");
    }
}
