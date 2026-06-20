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
        let name = raw.name.ok_or_else(|| de::Error::custom("missing required field: name"))?;
        let cases = raw.cases.ok_or_else(|| de::Error::custom("missing required field: cases"))?;

        if raw.inputs.is_some() && raw.operations.is_some() {
            return Err(de::Error::custom("inputs and operations are mutually exclusive"));
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
                let target = raw.target.ok_or_else(|| de::Error::missing_field("target"))?;
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
    pub binding: Option<BindingEntry>,
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    #[serde(default)]
    pub expected: BTreeMap<String, Value>,
    #[serde(default)]
    pub steps: Vec<TestStep>,
    #[serde(default)]
    pub postconditions: Option<Vec<Postcondition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Postcondition {
    pub target: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    #[serde(default)]
    pub desc: Option<String>,
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

/// Validates a YAML string as a spec document. Returns `Ok(())` if valid,
/// `Err(reason)` if invalid.
pub fn validate_spec_document(yaml: &str) -> Result<SpecDocument, String> {
    let doc: SpecDocument = serde_yaml::from_str(yaml).map_err(|e| format!("parse error: {e}"))?;

    validate_case_fields(&doc)?;

    Ok(doc)
}

fn validate_case_fields(doc: &SpecDocument) -> Result<(), String> {
    if doc.inputs.is_empty() && doc.outputs.is_empty() {
        return Ok(());
    }

    for case in &doc.cases {
        if !doc.inputs.is_empty() {
            for key in case.inputs.keys() {
                if key.starts_with("mock_") {
                    continue;
                }
                if !doc.inputs.contains_key(key) {
                    return Err(format!("case input '{key}' is not declared in spec inputs"));
                }
            }
        }

        if !doc.outputs.is_empty() {
            let declared_outputs = resolve_outcome_outputs(doc, case);
            for key in case.expected.keys() {
                if key == "outcome" {
                    continue;
                }
                if !declared_outputs.contains(&key.to_string()) {
                    return Err(format!("case expected field '{key}' is not declared in spec outputs"));
                }
            }
        }
    }

    Ok(())
}

fn resolve_outcome_outputs(doc: &SpecDocument, case: &SpecCase) -> Vec<String> {
    let outcome = case.expected.get("outcome").and_then(|v| v.as_str()).unwrap_or("");

    let when_key = format!("when {outcome}");
    if let Some(block) = doc.outputs.get(&when_key) {
        if let Some(map) = block.as_mapping() {
            return map.keys().filter_map(|k| k.as_str().map(String::from)).collect();
        }
    }

    doc.outputs
        .values()
        .filter_map(|v| v.as_mapping())
        .flat_map(|m| m.keys())
        .filter_map(|k| k.as_str().map(String::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{BindingDecl, BindingEntry, SpecDocument, validate_spec_document};

    #[test]
    fn validate_accepts_valid_spec() {
        let yaml = "name: test.x\nbinding:\n  name: rust\n  target: test\ninputs:\n  a: { type: int }\noutcome: Ok\noutputs:\n  when Ok:\n    result: int\ncases:\n  - name: c1\n    desc: d\n    inputs:\n      a: 1\n    expected:\n      outcome: Ok\n      result: 5\n";
        assert!(validate_spec_document(yaml).is_ok());
    }

    #[test]
    fn validate_rejects_undeclared_input() {
        let yaml = "name: test.x\nbinding:\n  name: rust\n  target: test\ninputs:\n  a: { type: int }\noutcome: Ok\noutputs:\n  when Ok:\n    result: int\ncases:\n  - name: c1\n    desc: d\n    inputs:\n      a: 1\n      x: 99\n    expected:\n      outcome: Ok\n      result: 5\n";
        let err = validate_spec_document(yaml).unwrap_err();
        assert!(err.contains("case input 'x' is not declared"), "{err}");
    }

    #[test]
    fn validate_rejects_undeclared_expected() {
        let yaml = "name: test.x\nbinding:\n  name: rust\n  target: test\ninputs:\n  a: { type: int }\noutcome: Ok\noutputs:\n  when Ok:\n    result: int\ncases:\n  - name: c1\n    desc: d\n    inputs:\n      a: 1\n    expected:\n      outcome: Ok\n      result: 5\n      extra: 42\n";
        let err = validate_spec_document(yaml).unwrap_err();
        assert!(err.contains("case expected field 'extra' is not declared"), "{err}");
    }

    #[test]
    fn validate_skips_when_no_inputs_declared() {
        let yaml = "name: test.x\noutcome: Ok\ncases:\n  - name: c1\n    desc: d\n    inputs:\n      anything: 1\n    expected:\n      outcome: Ok\n";
        assert!(validate_spec_document(yaml).is_ok());
    }

    #[test]
    fn deserializes_legacy_binding_format() {
        let spec: SpecDocument = serde_yaml::from_str("name: legacy\nbinding: rust\ntarget: test\noutcome: Ok\ncases: []\n")
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
        let spec: SpecDocument = serde_yaml::from_str("name: modern\nbinding:\n  - name: rust\n    target: test\noutcome: Ok\ncases: []\n")
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

    #[test]
    fn deserializes_case_postconditions() {
        let spec: SpecDocument = serde_yaml::from_str(
            r#"
name: test.machine
state:
  count: int
init:
  count: 0
operations:
  increment:
    inputs:
      amount:
        type: int
cases:
  - name: cleanup_verified
    desc: Verifies cleanup after the run
    steps:
      - operation: increment
        inputs: { amount: 1 }
        assert_state: { count: 1 }
    postconditions:
      - target: assert-file-absent
        inputs:
          path: "{generated_test_path}"
        desc: generated file removed
"#,
        )
        .expect("spec with postconditions should deserialize");

        let postconditions = spec.cases[0].postconditions.as_ref().expect("case should include postconditions");
        assert_eq!(postconditions.len(), 1);
        assert_eq!(postconditions[0].target, "assert-file-absent");
        assert_eq!(
            postconditions[0].inputs.get("path").map(String::as_str),
            Some("{generated_test_path}")
        );
        assert_eq!(postconditions[0].desc.as_deref(), Some("generated file removed"));
    }
}
