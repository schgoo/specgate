//! Checked reference-to-storage bindings and conversion of actual capture values.

use super::{BTreeMap, Candidate, Capture, Error, Link, ReplayBundle, Schema, failure};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use specgate::__rt::{Completion, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Bindings {
    nodes: Vec<Node>,
    operations: Vec<Operation>,
    rejections: Vec<Rejection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Rejection {
    component: String,
    name: String,
    diagnostic: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Operation {
    component: String,
    name: String,
    inputs: BTreeMap<String, usize>,
    result: Option<usize>,
    errors: BTreeMap<String, Option<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Node {
    Identity,
    Sequence {
        item: usize,
    },
    Tuple {
        items: Vec<usize>,
    },
    Map {
        keys: usize,
        values: usize,
        pairs: bool,
        compact: bool,
    },
    Record {
        fields: BTreeMap<String, usize>,
    },
    Tagged {
        variants: BTreeMap<String, Option<usize>>,
    },
    Optional {
        value: usize,
    },
}

pub(super) fn build(bundle: &ReplayBundle, candidate: &Candidate, links: &[Link]) -> Result<Bindings, Error> {
    let reference: Json = serde_json::from_slice(&bundle.registry_document)
        .map_err(|error| format!("failed to inspect linked reference registry: {error}"))?;
    let mut components = BTreeMap::<_, Vec<_>>::new();
    for operation in &bundle.registry.operations {
        let component = operation.component_id().as_str();
        let name = operation.name().as_str();
        let linked = links
            .iter()
            .any(|link| link.component_id.as_str() == component && link.operation_name.as_str() == name);
        if !declares(candidate, component, name) {
            if linked {
                return failure(format!("candidate is missing operation '{component}::{name}'"));
            }
            continue;
        }
        let observed = bundle
            .observed_operations
            .iter()
            .any(|observed| observed.component_id == *operation.component_id() && observed.operation_name == *operation.name());
        components.entry(component).or_default().push((operation, linked || observed));
    }
    let mut bindings = Bindings::default();
    for (component, operations) in components {
        let (schema, actual) = match candidate_registry(candidate, component) {
            Ok(declarations) => declarations,
            Err(error) => {
                if operations.iter().any(|(_, required)| *required) {
                    return Err(error);
                }
                for (operation, _) in operations {
                    bindings.reject(component, operation.name().as_str(), &error);
                }
                continue;
            }
        };
        for (operation, required) in operations {
            let checkpoint = bindings.nodes.len();
            let mut builder = Builder {
                reference: &reference,
                candidate: &actual,
                bindings,
                visited: BTreeMap::new(),
            };
            let result = builder.operation(operation, &schema);
            bindings = builder.bindings;
            if let Err(error) = result {
                if required {
                    return Err(error);
                }
                // Unused declarations do not block planning or leave partial conversion nodes.
                bindings.nodes.truncate(checkpoint);
                bindings.reject(component, operation.name().as_str(), &error);
            }
        }
    }
    Ok(bindings)
}

fn declares(candidate: &Candidate, component: &str, name: &str) -> bool {
    let schema = if candidate.schema.component.as_str() == component {
        Some(&candidate.schema)
    } else {
        candidate.metadata().schemas.get(component)
    };
    match schema {
        Some(schema) => schema.operations.iter().any(|operation| operation.name.as_str() == name),
        None => candidate
            .metadata()
            .raw_registry
            .ops
            .iter()
            .any(|operation| !operation.is_setup && operation.component.as_str() == component && operation.name.as_str() == name),
    }
}

fn candidate_registry(candidate: &Candidate, component: &str) -> Result<(Schema, Json), Error> {
    let schema = if candidate.schema.component.as_str() == component {
        candidate.schema.clone()
    } else {
        match candidate.metadata().schemas.get(component) {
            Some(schema) => schema.clone(),
            None => {
                specgate_discovery::schema::normalize_registry(&candidate.metadata().raw_registry, candidate.metadata().language, component)
                    .map_err(|error| format!("failed to normalize candidate component '{component}': {error}"))?
            }
        }
    };
    let document = serde_json::to_string(&schema).map_err(|error| format!("failed to encode candidate type schema: {error}"))?;
    let encoded = specgate_ctsc::registry::encode(
        "urn:ctsc:registry:replay-types",
        "0.1.0",
        specgate_ctsc::registry::Schema::new(document),
    )
    .map_err(|error| format!("failed to normalize candidate declarations: {error}"))?;
    let actual =
        serde_json::from_str(&encoded.registry_json).map_err(|error| format!("failed to inspect candidate declarations: {error}"))?;
    Ok((schema, actual))
}

struct Builder<'a> {
    reference: &'a Json,
    candidate: &'a Json,
    bindings: Bindings,
    visited: BTreeMap<(String, String, String), usize>,
}

impl Builder<'_> {
    fn operation(&mut self, operation: &specgate_ctsc::replay::model::RegistryOp, schema: &Schema) -> Result<(), Error> {
        let component = operation.component_id().as_str();
        let name = operation.name().as_str();
        let found = component_member(self.candidate, component, "operations", name)
            .ok_or_else(|| format!("candidate is missing operation '{component}::{name}'"))?;
        let normalized = schema
            .operations
            .iter()
            .find(|operation| operation.name.as_str() == name)
            .expect("encoded candidate operation has a normalized declaration");
        super::link::validate_inputs(operation, normalized)?;
        super::link::validate_output(operation, normalized)?;
        let mut inputs = BTreeMap::new();
        for input in operation.inputs() {
            let native = found["inputs"]
                .as_array()
                .and_then(|inputs| inputs.iter().find(|field| field["name"] == input.name))
                .ok_or_else(|| format!("candidate operation '{component}::{name}' input names differ"))?;
            let expected = serde_json::to_value(&input.value_type).expect("replay types serialize");
            let id = self.bind(&expected, &native["type"], component, &format!("{name} input '{}'", input.name))?;
            inputs.insert(input.name.clone(), id);
        }
        let result = operation
            .output()
            .map(|ty| {
                let expected = serde_json::to_value(ty).expect("replay types serialize");
                self.bind(&expected, &found["outcomes"]["result"], component, &format!("{name} output"))
            })
            .transpose()?;
        let mut errors = BTreeMap::new();
        for error in operation.errors() {
            let native = found["outcomes"]["errors"]
                .as_array()
                .and_then(|errors| errors.iter().find(|entry| entry["name"] == error.name))
                .ok_or_else(|| format!("candidate operation '{component}::{name}' declared errors mismatch"))?;
            let payload = error
                .value_type
                .as_ref()
                .map(|ty| {
                    let expected = serde_json::to_value(ty).expect("replay types serialize");
                    self.bind(&expected, &native["type"], component, &format!("{name} error '{}'", error.name))
                })
                .transpose()?;
            errors.insert(error.name.clone(), payload);
        }
        self.bindings.operations.push(Operation {
            component: component.to_string(),
            name: name.to_string(),
            inputs,
            result,
            errors,
        });
        Ok(())
    }

    fn bind(&mut self, expected: &Json, actual: &Json, component: &str, location: &str) -> Result<usize, Error> {
        let key = (component.to_string(), expected.to_string(), actual.to_string());
        if let Some(id) = self.visited.get(&key) {
            return Ok(*id);
        }
        let id = self.bindings.nodes.len();
        self.visited.insert(key, id);
        // Reserve before following named declarations so recursive types share one node.
        self.bindings.nodes.push(Node::Identity);
        let node = self.shape(expected, actual, component, location).map_err(|error| {
            if expected["kind"] == "primitive" {
                error
            } else {
                Error::from(format!(
                    "candidate uses unsupported structured replay type mapping at '{component}::{location}': {error}"
                ))
            }
        })?;
        self.bindings.nodes[id] = node;
        Ok(id)
    }

    fn shape(&mut self, expected: &Json, actual: &Json, component: &str, location: &str) -> Result<Node, Error> {
        match (expected["kind"].as_str(), actual["kind"].as_str()) {
            (Some("primitive"), Some("primitive")) if expected == actual => Ok(Node::Identity),
            (Some("named"), Some("named")) => {
                let expected_owner = expected["componentId"].as_str().unwrap_or(component);
                let actual_owner = actual["componentId"].as_str().unwrap_or(component);
                if expected["name"] != actual["name"]
                    || expected_owner != actual_owner
                    || expected.get("registryId").is_some()
                    || actual.get("registryId").is_some()
                {
                    return mismatch(expected, actual, component, location);
                }
                let name = expected["name"].as_str().expect("verified named type");
                let left = component_member(self.reference, expected_owner, "types", name)
                    .ok_or_else(|| format!("reference type '{expected_owner}::{name}' has no linked declaration"))?;
                let right = component_member(self.candidate, actual_owner, "types", name)
                    .ok_or_else(|| format!("candidate type '{actual_owner}::{name}' has no linked declaration"))?;
                self.shape(left, right, expected_owner, &format!("{location}.{name}"))
            }
            (Some("record"), Some("named")) if actual.get("registryId").is_none() => {
                let owner = actual["componentId"].as_str().unwrap_or(component);
                let name = actual["name"].as_str().expect("verified named type");
                let declaration = component_member(self.candidate, owner, "types", name)
                    .ok_or_else(|| format!("candidate type '{owner}::{name}' has no linked declaration"))?;
                if declaration["kind"] != "record" || owner != component {
                    return mismatch(expected, actual, component, location);
                }
                self.shape(expected, declaration, component, location)
            }
            (Some("collection"), Some("collection"))
                if (actual["ordered"] == true && actual["unique"] == false)
                    || (expected["ordered"] == false
                        && expected["unique"] == true
                        && actual["ordered"] == false
                        && actual["unique"] == true) =>
            {
                Ok(Node::Sequence {
                    item: self.bind(&expected["items"], &actual["items"], component, &format!("{location}[]"))?,
                })
            }
            (Some("optional"), Some("optional")) => Ok(Node::Optional {
                value: self.bind(&expected["value"], &actual["value"], component, &format!("{location}.Some"))?,
            }),
            (Some("map"), Some("map")) if expected["ordered"] == false && actual["ordered"] == false => Ok(Node::Map {
                keys: self.bind(&expected["keys"], &actual["keys"], component, &format!("{location}.key"))?,
                values: self.bind(&expected["values"], &actual["values"], component, &format!("{location}.value"))?,
                pairs: false,
                compact: expected["keys"]["kind"] == "primitive" && expected["keys"]["name"] == "string",
            }),
            (Some("map"), Some("collection")) if actual["ordered"] == true && actual["unique"] == false => {
                let Some(items) = actual["items"]["items"].as_array().filter(|items| items.len() == 2) else {
                    return mismatch(expected, actual, component, location);
                };
                if actual["items"]["kind"] != "tuple" {
                    return mismatch(expected, actual, component, location);
                }
                Ok(Node::Map {
                    keys: self.bind(&expected["keys"], &items[0], component, &format!("{location}.key"))?,
                    values: self.bind(&expected["values"], &items[1], component, &format!("{location}.value"))?,
                    pairs: true,
                    compact: expected["ordered"] == false
                        && expected["keys"]["kind"] == "primitive"
                        && expected["keys"]["name"] == "string",
                })
            }
            (Some("tuple"), Some("tuple")) => {
                if !expected["items"].as_array().is_some_and(|items| (2..=4).contains(&items.len())) {
                    return mismatch(expected, actual, component, location);
                }
                self.tuple(expected, actual, component, location)
            }
            (Some("record"), Some("record")) => {
                let left = expected["fields"].as_array().expect("verified record fields");
                let right = actual["fields"].as_array().expect("normalized record fields");
                if left.len() != right.len() {
                    return mismatch(expected, actual, component, location);
                }
                let mut fields = BTreeMap::new();
                for field in left {
                    let name = field["name"].as_str().expect("verified field name");
                    let Some(native) = right.iter().find(|native| native["name"] == name) else {
                        return mismatch(expected, actual, component, location);
                    };
                    fields.insert(
                        name.to_string(),
                        self.bind(&field["type"], &native["type"], component, &format!("{location}.{name}"))?,
                    );
                }
                Ok(Node::Record { fields })
            }
            (Some("tagged_union"), Some("tagged_union")) => {
                let left = expected["variants"].as_array().expect("verified variants");
                let right = actual["variants"].as_array().expect("normalized variants");
                if left.len() != right.len() {
                    return mismatch(expected, actual, component, location);
                }
                let mut variants = BTreeMap::new();
                for variant in left {
                    let name = variant["name"].as_str().expect("verified variant name");
                    let Some(native) = right.iter().find(|native| native["name"] == name) else {
                        return mismatch(expected, actual, component, location);
                    };
                    let payload = match (variant.get("payload"), native.get("payload")) {
                        (None, None) => None,
                        (Some(left), Some(right)) if left["kind"] == "tuple" && right["kind"] == "tuple" => {
                            let node = self.tuple(left, right, component, &format!("{location}.{name}"))?;
                            let id = self.bindings.nodes.len();
                            self.bindings.nodes.push(node);
                            Some(id)
                        }
                        (Some(left), Some(right)) => Some(self.bind(left, right, component, &format!("{location}.{name}"))?),
                        _ => return mismatch(expected, actual, component, location),
                    };
                    variants.insert(name.to_string(), payload);
                }
                Ok(Node::Tagged { variants })
            }
            _ => mismatch(expected, actual, component, location),
        }
    }

    fn tuple(&mut self, expected: &Json, actual: &Json, component: &str, location: &str) -> Result<Node, Error> {
        let left = expected["items"].as_array().expect("verified tuple items");
        let right = actual["items"].as_array().expect("normalized tuple items");
        if left.len() != right.len() {
            return mismatch(expected, actual, component, location);
        }
        Ok(Node::Tuple {
            items: left
                .iter()
                .zip(right)
                .enumerate()
                .map(|(index, (left, right))| self.bind(left, right, component, &format!("{location}[{index}]")))
                .collect::<Result<_, _>>()?,
        })
    }
}

fn component_member<'a>(registry: &'a Json, component: &str, member: &str, name: &str) -> Option<&'a Json> {
    registry["components"].as_array()?.iter().find(|entry| entry["id"] == component)?[member]
        .as_array()?
        .iter()
        .find(|entry| entry["name"] == name)
}

fn mismatch<T>(expected: &Json, actual: &Json, component: &str, location: &str) -> Result<T, Error> {
    let category = if expected["kind"] == "primitive" {
        "type mismatch"
    } else {
        "uses unsupported structured replay type mapping"
    };
    let (operation, value) = location.split_once(' ').unwrap_or((location, "value"));
    failure(format!(
        "candidate operation '{component}::{operation}' {value} {category}: expected registry type {expected}, found storage type {actual}"
    ))
}

impl Bindings {
    fn reject(&mut self, component: &str, name: &str, error: &Error) {
        self.rejections.push(Rejection {
            component: component.to_string(),
            name: name.to_string(),
            diagnostic: error.to_string(),
        });
    }

    pub(super) fn convert(&self, captures: &[Capture]) -> Result<Vec<Capture>, Error> {
        let mut captures = captures.to_vec();
        for capture in &mut captures {
            for operation in &mut capture.operations {
                if let Some(rejection) = self.rejections.iter().find(|rejection| {
                    rejection.component == operation.component_id.as_str() && rejection.name == operation.operation_name.as_str()
                }) {
                    return failure(format!(
                        "candidate operation '{}::{}' has no compatible storage binding: {}",
                        rejection.component, rejection.name, rejection.diagnostic
                    ));
                }
                let Some(binding) = self.operations.iter().find(|binding| {
                    binding.component == operation.component_id.as_str() && binding.name == operation.operation_name.as_str()
                }) else {
                    // Calls without a candidate declaration still undergo linked validation.
                    continue;
                };
                for (name, value) in &mut operation.inputs {
                    if let Some(id) = binding.inputs.get(name) {
                        *value = self.at(*id, value, binding, &format!("input '{name}'"))?;
                    }
                }
                match &mut operation.completion {
                    Some(Completion::Result { value, .. }) => {
                        if let Some(id) = binding.result {
                            *value = self.at(id, value, binding, "result")?;
                        }
                    }
                    Some(Completion::Error {
                        name, value: Some(value), ..
                    }) => {
                        if let Some(Some(id)) = binding.errors.get(name) {
                            *value = self.at(*id, value, binding, &format!("error '{name}'"))?;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(captures)
    }

    fn at(&self, id: usize, value: &Value, operation: &Operation, location: &str) -> Result<Value, Error> {
        self.value(id, value).map_err(|error| {
            Error::from(format!(
                "candidate operation '{}::{}' {location} conversion failed: {error}",
                operation.component, operation.name
            ))
        })
    }

    fn value(&self, id: usize, value: &Value) -> Result<Value, Error> {
        match &self.nodes[id] {
            Node::Identity => Ok(value.clone()),
            Node::Sequence { item } => {
                let values = sequence(value)?;
                values
                    .into_iter()
                    .map(|value| self.value(*item, value))
                    .collect::<Result<_, _>>()
                    .map(Value::List)
            }

            Node::Tuple { items } => {
                let values = sequence(value)?;
                if values.len() != items.len() {
                    return failure("candidate tuple length differs from its checked binding");
                }
                items
                    .iter()
                    .zip(values)
                    .map(|(id, value)| self.value(*id, value))
                    .collect::<Result<_, _>>()
                    .map(Value::List)
            }
            Node::Map {
                keys,
                values,
                pairs,
                compact,
            } => {
                let entries = if *pairs {
                    sequence(value)?
                        .into_iter()
                        .map(|entry| {
                            let fields = sequence(entry)?;
                            match fields.as_slice() {
                                [key, value] => Ok((*key, *value)),
                                _ => failure("candidate map pair must contain exactly a key and a value"),
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    let Value::Map(entries) = value else {
                        return failure("candidate native map must contain string keys");
                    };
                    let converted = entries
                        .iter()
                        .map(|(key, value)| Ok((Value::String(key.clone()), self.value(*values, value)?)))
                        .collect::<Result<Vec<_>, Error>>()?;
                    return map_value(converted, *compact);
                };
                let converted = entries
                    .into_iter()
                    .map(|(key, value)| Ok((self.value(*keys, key)?, self.value(*values, value)?)))
                    .collect::<Result<Vec<_>, Error>>()?;
                map_value(converted, *compact)
            }
            Node::Record { fields } => {
                let Value::Map(values) = value else {
                    return failure("candidate record must contain named fields");
                };
                values
                    .iter()
                    .map(|(name, value)| {
                        Ok((
                            name.clone(),
                            fields.get(name).map_or_else(|| Ok(value.clone()), |id| self.value(*id, value))?,
                        ))
                    })
                    .collect::<Result<_, Error>>()
                    .map(Value::Map)
            }
            Node::Tagged { variants } => {
                let Value::Map(values) = value else {
                    return failure("candidate tagged union must contain a named variant");
                };
                values
                    .iter()
                    .map(|(name, value)| {
                        let value = match variants.get(name) {
                            Some(Some(id)) => self.value(*id, value)?,
                            _ => value.clone(),
                        };
                        Ok((name.clone(), value))
                    })
                    .collect::<Result<_, Error>>()
                    .map(Value::Map)
            }
            Node::Optional { value: id } => {
                let Value::Map(values) = value else {
                    return failure("candidate optional must contain a Some/None tag");
                };
                values
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), if name == "Some" { self.value(*id, value)? } else { value.clone() })))
                    .collect::<Result<_, Error>>()
                    .map(Value::Map)
            }
        }
    }
}

fn sequence(value: &Value) -> Result<Vec<&Value>, Error> {
    match value {
        Value::List(values) => Ok(values.iter().collect()),
        Value::Set(values) => Ok(values.iter().collect()),
        _ => failure("candidate collection or tuple must contain an array"),
    }
}

fn map_value(entries: Vec<(Value, Value)>, compact: bool) -> Result<Value, Error> {
    if compact {
        let mut values = BTreeMap::new();
        for (key, value) in entries {
            let Value::String(key) = key else {
                return failure("candidate map key must be a string");
            };
            if values.insert(key, value).is_some() {
                return failure("candidate map contains duplicate keys; values must not be overwritten");
            }
        }
        Ok(Value::Map(values))
    } else {
        // Keep every entry. Linked validation checks recursive CTSC key equality.
        Ok(Value::List(
            entries
                .into_iter()
                .map(|(key, value)| Value::Map(BTreeMap::from([("key".to_string(), key), ("value".to_string(), value)])))
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registry(types: Json) -> Json {
        let mut registry = json!({"components":[{"id":"fixture.bindings"}]});
        registry["components"][0]["types"] = types;
        registry
    }

    fn builder<'a>(reference: &'a Json, candidate: &'a Json) -> Builder<'a> {
        Builder {
            reference,
            candidate,
            bindings: Bindings::default(),
            visited: BTreeMap::new(),
        }
    }

    #[test]
    fn inline_record_matching_is_one_way_and_keeps_exact_fields_and_named_identity() {
        let fields = json!([
            {"name":"key","type":{"kind":"primitive","name":"string"}},
            {"name":"items","type":{"kind":"collection","ordered":false,"unique":false,"items":{"kind":"primitive","name":"i32"}}}
        ]);
        let inline = json!({"kind":"record","fields":fields});
        let mut native = inline.clone();
        native["name"] = json!("Grouping");
        native["fields"][1]["type"]["ordered"] = json!(true);
        let named = json!({"kind":"named","name":"Grouping"});
        let reference = registry(json!([]));
        let candidate = registry(json!([native.clone()]));
        let mut bindings = builder(&reference, &candidate);
        let id = bindings.bind(&inline, &named, "fixture.bindings", "grouping output").unwrap();
        let value = Value::Map(BTreeMap::from([
            ("key".to_string(), Value::String("group".to_string())),
            (
                "items".to_string(),
                Value::List(vec![Value::Integer(2), Value::Integer(1), Value::Integer(2)]),
            ),
        ]));
        assert_eq!(bindings.bindings.value(id, &value).unwrap(), value);
        builder(&candidate, &reference)
            .bind(&named, &inline, "fixture.bindings", "reverse output")
            .unwrap_err();
        let renamed = json!({"kind":"named","name":"OtherGrouping"});
        builder(&candidate, &candidate)
            .bind(&named, &renamed, "fixture.bindings", "renamed output")
            .unwrap_err();
        native["fields"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"extra","type":{"kind":"primitive","name":"bool"}}));
        let extra = registry(json!([native]));
        builder(&reference, &extra)
            .bind(&inline, &named, "fixture.bindings", "extra output")
            .unwrap_err();
    }

    #[test]
    fn recursive_named_bindings_convert_nested_pair_maps_without_losing_entries() {
        let named = json!({"kind":"named","name":"Node"});
        let reference = registry(json!([{"name":"Node","kind":"record","fields":[
            {"name":"children","type":{"kind":"collection","ordered":false,"unique":false,"items":named}},
            {"name":"entries","type":{"kind":"map","ordered":true,"keys":{"kind":"primitive","name":"i32"},"values":{"kind":"primitive","name":"bool"}}}
        ]}]));
        let mut candidate = reference.clone();
        candidate["components"][0]["types"][0]["fields"][0]["type"]["ordered"] = json!(true);
        candidate["components"][0]["types"][0]["fields"][1]["type"] = json!({
            "kind":"collection","ordered":true,"unique":false,
            "items":{"kind":"tuple","items":[{"kind":"primitive","name":"i32"},{"kind":"primitive","name":"bool"}]}
        });
        let mut bindings = builder(&reference, &candidate);
        let id = bindings.bind(&named, &named, "fixture.bindings", "tree output").unwrap();
        let node = |children| {
            Value::Map(BTreeMap::from([
                ("children".to_string(), Value::List(children)),
                (
                    "entries".to_string(),
                    Value::List(vec![Value::List(vec![Value::Integer(2), Value::Bool(true)])]),
                ),
            ]))
        };
        let converted = bindings.bindings.value(id, &node(vec![node(Vec::new())])).unwrap();
        let Value::Map(root) = converted else {
            panic!("expected root record")
        };
        let Value::List(children) = &root["children"] else {
            panic!("expected child nodes")
        };
        let Value::Map(child) = &children[0] else {
            panic!("expected child record")
        };
        let expected = Value::List(vec![Value::Map(BTreeMap::from([
            ("key".to_string(), Value::Integer(2)),
            ("value".to_string(), Value::Bool(true)),
        ]))]);
        assert_eq!(root["entries"], expected);
        assert_eq!(child["entries"], expected);
    }
}
