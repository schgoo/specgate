//! Feasibility probe, not collection support: declarations are registered by
//! hand, and Rust supplies concrete identities through function signatures.
//! The resulting semantic types enter the real registry encoder and validator.

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use specgate_ctsc::registry::{Schema as EncodingSchema, encode};
use specgate_ctsc::validation::{DocumentBytes, bytes::validate_registry};
use specgate_discovery::schema::{Operation, Schema};
use std::any::type_name;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::Path;

mod unordered {
    pub struct Bag<T> {
        pub _items: Vec<T>,
    }
}

mod ordered {
    pub struct Bag<T> {
        pub _items: Vec<T>,
    }
}

struct Order;
struct Lookup<K, V>(PhantomData<(K, V)>);
struct Expanding<T>(PhantomData<T>);
struct Indirect<T>(PhantomData<T>);

#[derive(Clone, Debug)]
struct Native {
    path: String,
    arguments: Vec<Self>,
}

impl Native {
    fn of<T>() -> Self {
        Self::parse(type_name::<T>())
    }

    // Only the named-type/generic subset used by this probe, not a Rust parser.
    fn parse(name: &str) -> Self {
        let Some((path, tail)) = name.split_once('<') else {
            return Self {
                path: name.to_owned(),
                arguments: Vec::new(),
            };
        };
        let inner = tail.strip_suffix('>').expect("compiler-produced generic type");
        let mut depth = 0;
        let mut start = 0;
        let mut arguments = Vec::new();
        for (index, character) in inner.char_indices() {
            match character {
                '<' => depth += 1,
                '>' => depth -= 1,
                ',' if depth == 0 => {
                    arguments.push(Self::parse(inner[start..index].trim()));
                    start = index + 1;
                }
                _ => {}
            }
        }
        arguments.push(Self::parse(inner[start..].trim()));
        Self {
            path: path.to_owned(),
            arguments,
        }
    }
}

enum Expression {
    Parameter(usize),
    Apply { path: String, arguments: Vec<Self> },
}

impl Expression {
    fn apply<T>(arguments: Vec<Self>) -> Self {
        Self::Apply {
            path: Native::of::<T>().path,
            arguments,
        }
    }
}

enum Shape {
    Reference(&'static str),
    Collection {
        items: Expression,
        ordered: bool,
        unique: bool,
    },
    Map {
        keys: Expression,
        values: Expression,
        ordered: bool,
    },
}

struct Declaration {
    parameters: usize,
    shape: Shape,
}

#[derive(Default)]
struct Resolver(BTreeMap<String, Declaration>);

impl Resolver {
    fn register<T>(&mut self, shape: Shape) -> Result<(), String> {
        let native = Native::of::<T>();
        if self.0.contains_key(&native.path) {
            return Err(format!("duplicate native declaration: {}", native.path));
        }
        self.0.insert(
            native.path,
            Declaration {
                parameters: native.arguments.len(),
                shape,
            },
        );
        Ok(())
    }

    fn resolve(&self, native: &Native) -> Result<String, String> {
        // Resolve finite use-site nesting before following declaration edges.
        // A<A<i32>> is finite; A<T> declaring A<Vec<T>> is not.
        let arguments = native
            .arguments
            .iter()
            .map(|argument| self.resolve(argument))
            .collect::<Result<Vec<_>, _>>()?;
        self.instantiate(&native.path, &arguments, &mut Vec::new())
    }

    fn instantiate(&self, path: &str, arguments: &[String], active: &mut Vec<String>) -> Result<String, String> {
        let declaration = self.0.get(path).ok_or_else(|| format!("unknown native declaration: {path}"))?;
        if declaration.parameters != arguments.len() {
            return Err(format!("wrong generic arity: {path}"));
        }
        if active.iter().any(|entry| entry == path) {
            return Err(format!("expanding declaration: {} -> {path}", active.join(" -> ")));
        }
        active.push(path.to_owned());
        let result = match &declaration.shape {
            Shape::Reference(name) => Ok((*name).to_owned()),
            Shape::Collection { items, ordered, unique } => self.expression(items, arguments, active).map(|items| {
                let constructor = match (ordered, unique) {
                    (true, false) => "List",
                    (false, false) => "Bag",
                    (false, true) => "set",
                    (true, true) => "OrderedSet",
                };
                format!("{constructor}<{items}>")
            }),
            Shape::Map { keys, values, ordered } => {
                let keys = self.expression(keys, arguments, active);
                let values = self.expression(values, arguments, active);
                keys.and_then(|keys| {
                    values.map(|values| {
                        let constructor = if *ordered { "OrderedMap" } else { "map" };
                        format!("{constructor}<{keys}, {values}>")
                    })
                })
            }
        };
        active.pop();
        result
    }

    fn expression(&self, expression: &Expression, arguments: &[String], active: &mut Vec<String>) -> Result<String, String> {
        match expression {
            Expression::Parameter(index) => arguments.get(*index).cloned().ok_or_else(|| "unbound generic parameter".to_owned()),
            Expression::Apply {
                path,
                arguments: expressions,
            } => {
                let arguments = expressions
                    .iter()
                    .map(|expression| self.expression(expression, arguments, active))
                    .collect::<Result<Vec<_>, _>>()?;
                self.instantiate(path, &arguments, active)
            }
        }
    }
}

fn resolver() -> Resolver {
    use Expression::Parameter;
    let mut resolver = Resolver::default();
    resolver.register::<String>(Shape::Reference("string")).unwrap();
    resolver.register::<i32>(Shape::Reference("i32")).unwrap();
    resolver.register::<Order>(Shape::Reference("Order")).unwrap();
    for (path, ordered, unique) in [
        (Native::of::<unordered::Bag<()>>(), false, false),
        (Native::of::<ordered::Bag<()>>(), true, true),
        (Native::of::<Vec<()>>(), true, false),
    ] {
        resolver.0.insert(
            path.path,
            Declaration {
                parameters: 1,
                shape: Shape::Collection {
                    items: Parameter(0),
                    ordered,
                    unique,
                },
            },
        );
    }
    resolver
        .register::<Lookup<(), ()>>(Shape::Map {
            keys: Parameter(0),
            values: Parameter(1),
            ordered: true,
        })
        .unwrap();
    resolver
}

fn output<T>(_: fn() -> T) -> Native {
    Native::of::<T>()
}

fn orders() -> unordered::Bag<Order> {
    unordered::Bag { _items: vec![Order] }
}

fn labels() -> unordered::Bag<String> {
    unordered::Bag {
        _items: vec!["label".to_owned()],
    }
}

fn encoded_outputs(resolver: &Resolver, outputs: &[(&str, Native)]) -> Value {
    let raw = specgate_discovery::registry::Registry::parse(
        r#"{"operations":[],"types":[{"name":"Order","kind":"struct","component":"proof","fields":[]}]}"#,
    )
    .unwrap();
    let mut schema: Schema =
        specgate_discovery::schema::normalize_registry(&raw, specgate_discovery::binding::Language::Rust, "proof").unwrap();
    for (name, native) in outputs {
        schema
            .operations
            .push(Operation::builder(*name).output(resolver.resolve(native).unwrap()).build());
    }
    let encoded = encode("proof", "1", EncodingSchema::new(serde_json::to_string(&schema).unwrap())).unwrap();
    let report = validate_registry(
        DocumentBytes::new(Path::new("collection-metadata-proof.json"), encoded.registry_json.as_bytes()),
        [],
    );
    assert!(report.valid, "{report:#?}");
    assert_eq!(encoded.type_count, 1, "native containers must not become named CTSC wrappers");
    serde_json::from_str(&encoded.registry_json).unwrap()
}

#[test]
fn concrete_generic_outputs_preserve_named_items_without_container_wrappers() {
    let registry = encoded_outputs(&resolver(), &[("orders", output(orders)), ("labels", output(labels))]);
    let operations = registry["components"][0]["operations"].as_array().unwrap();
    let orders = operations.iter().find(|operation| operation["name"] == "orders").unwrap();
    let labels = operations.iter().find(|operation| operation["name"] == "labels").unwrap();
    assert_eq!(
        orders["outcomes"]["result"],
        json!({"kind":"collection","ordered":false,"unique":false,"items":{"kind":"named","name":"Order"}})
    );
    assert_eq!(
        labels["outcomes"]["result"],
        json!({"kind":"collection","ordered":false,"unique":false,"items":{"kind":"primitive","name":"string"}})
    );
}

#[test]
fn scoped_names_and_import_aliases_keep_distinct_shapes() {
    use ordered::Bag as Imported;
    type Alias<T> = Imported<T>;
    let resolver = resolver();
    let bag = resolver.resolve(&Native::of::<unordered::Bag<Order>>()).unwrap();
    let ordered = resolver.resolve(&Native::of::<Alias<Order>>()).unwrap();
    assert_eq!(bag, "Bag<Order>");
    assert_eq!(ordered, "OrderedSet<Order>");
    let registry = encoded_outputs(
        &resolver,
        &[
            ("unordered", Native::of::<unordered::Bag<Order>>()),
            ("ordered", Native::of::<Alias<Order>>()),
        ],
    );
    let operations = registry["components"][0]["operations"].as_array().unwrap();
    assert!(operations.iter().any(|operation| operation["outcomes"]["result"]["unique"] == true));
    assert!(
        operations
            .iter()
            .any(|operation| operation["outcomes"]["result"]["unique"] == false)
    );
}

#[test]
fn nested_specializations_and_multiple_parameters_substitute_recursively() {
    let resolver = resolver();
    assert_eq!(
        resolver
            .resolve(&Native::of::<Lookup<String, unordered::Bag<ordered::Bag<Order>>>>())
            .unwrap(),
        "OrderedMap<string, Bag<OrderedSet<Order>>>"
    );
    assert_eq!(
        resolver.resolve(&Native::of::<unordered::Bag<unordered::Bag<i32>>>()).unwrap(),
        "Bag<Bag<i32>>"
    );
    encoded_outputs(
        &resolver,
        &[("nested", Native::of::<Lookup<String, unordered::Bag<ordered::Bag<Order>>>>())],
    );
}

#[test]
fn declaration_templates_can_apply_nested_generic_constructors() {
    let mut resolver = resolver();
    resolver
        .register::<Expanding<()>>(Shape::Collection {
            items: Expression::apply::<ordered::Bag<()>>(vec![Expression::Parameter(0)]),
            ordered: false,
            unique: false,
        })
        .unwrap();
    assert_eq!(
        resolver.resolve(&Native::of::<Expanding<String>>()).unwrap(),
        "Bag<OrderedSet<string>>"
    );
    encoded_outputs(&resolver, &[("template", Native::of::<Expanding<Order>>())]);
}

#[test]
fn growing_and_indirect_declarations_fail_without_rejecting_finite_nesting() {
    let mut resolver = resolver();
    resolver
        .register::<Expanding<()>>(Shape::Collection {
            items: Expression::apply::<Expanding<()>>(vec![Expression::apply::<Vec<()>>(vec![Expression::Parameter(0)])]),
            ordered: false,
            unique: false,
        })
        .unwrap();
    let error = resolver.resolve(&Native::of::<Expanding<i32>>()).unwrap_err();
    assert!(error.starts_with("expanding declaration:"), "{error}");

    let mut resolver = self::resolver();
    resolver
        .register::<Expanding<()>>(Shape::Collection {
            items: Expression::apply::<Vec<()>>(vec![Expression::apply::<Indirect<()>>(vec![Expression::Parameter(0)])]),
            ordered: false,
            unique: false,
        })
        .unwrap();
    resolver
        .register::<Indirect<()>>(Shape::Collection {
            items: Expression::apply::<Expanding<()>>(vec![Expression::Parameter(0)]),
            ordered: true,
            unique: false,
        })
        .unwrap();
    let error = resolver.resolve(&Native::of::<Expanding<String>>()).unwrap_err();
    assert!(error.starts_with("expanding declaration:"), "{error}");
    assert_eq!(
        resolver.resolve(&Native::of::<unordered::Bag<unordered::Bag<Order>>>()).unwrap(),
        "Bag<Bag<Order>>"
    );
}

#[test]
fn missing_ambiguous_and_invalid_metadata_fail_closed() {
    let mut resolver = resolver();
    let error = resolver.resolve(&Native::of::<Expanding<i32>>()).unwrap_err();
    assert!(error.starts_with("unknown native declaration:"), "{error}");
    let error = resolver.resolve(&Native::parse("Bag<i32>")).unwrap_err();
    assert_eq!(
        error, "unknown native declaration: Bag",
        "never guess a declaration from a bare name"
    );
    let error = resolver.register::<String>(Shape::Reference("i32")).unwrap_err();
    assert!(error.starts_with("duplicate native declaration:"), "{error}");

    let error = resolver
        .resolve(&Native {
            path: Native::of::<unordered::Bag<()>>().path,
            arguments: Vec::new(),
        })
        .unwrap_err();
    assert!(error.starts_with("wrong generic arity:"), "{error}");
    resolver
        .register::<Expanding<()>>(Shape::Collection {
            items: Expression::Parameter(1),
            ordered: false,
            unique: false,
        })
        .unwrap();
    assert_eq!(
        resolver.resolve(&Native::of::<Expanding<i32>>()).unwrap_err(),
        "unbound generic parameter"
    );
}

fn qualified_registry() -> Value {
    let mut resolver = resolver();
    resolver.0.get_mut(&Native::of::<Order>().path).unwrap().shape = Shape::Reference("proof.models::Order");
    let schema = json!({
        "component": "proof.api",
        "dependencies": ["proof.models"],
        "dependency_types": [{
            "component": "proof.models",
            "types": [{"name": "Order", "kind": "struct", "fields": [{"name": "id", "ty": "i32"}]}]
        }],
        "types": [{"name": "Order", "kind": "struct", "fields": [{"name": "label", "ty": "string"}]}],
        "operations": [
            Operation::builder("orders")
                .output(resolver.resolve(&Native::of::<unordered::Bag<Order>>()).unwrap())
                .build()
        ]
    });
    let encoded = encode("urn:registry:api", "1", EncodingSchema::new(schema.to_string())).unwrap();
    assert_eq!(encoded.type_count, 2, "only the two component-owned records are named");
    serde_json::from_str(&encoded.registry_json).unwrap()
}

#[test]
fn collection_items_keep_cross_component_record_ownership() {
    let registry = qualified_registry();
    let bytes = serde_json::to_vec(&registry).unwrap();
    let report = validate_registry(DocumentBytes::new(Path::new("api.json"), &bytes), []);
    assert!(report.valid, "{report:#?}");
    assert_eq!(
        registry["components"][0]["operations"][0]["outcomes"]["result"],
        json!({
            "kind": "collection", "ordered": false, "unique": false,
            "items": {"kind": "named", "componentId": "proof.models", "name": "Order"}
        })
    );
    assert_eq!(registry["components"][0]["types"][0]["fields"][0]["name"], "label");
    assert_eq!(registry["components"][1]["types"][0]["fields"][0]["name"], "id");
}

#[test]
fn imported_collection_items_require_import_dependency_and_exact_bytes() {
    let mut root = qualified_registry();
    let models = root["components"].as_array_mut().unwrap().remove(1);
    let models = json!({
        "format": "ctsc.registry",
        "formatVersion": root["formatVersion"],
        "registryId": "urn:registry:models",
        "version": "1",
        "components": [models]
    });
    let models = serde_json::to_vec(&models).unwrap();
    root["imports"] = json!([{
        "registryId": "urn:registry:models", "version": "1",
        "digest": format!("sha256:{:x}", Sha256::digest(&models))
    }]);
    root["components"][0]["dependencies"][0]["registryId"] = json!("urn:registry:models");
    root["components"][0]["operations"][0]["outcomes"]["result"]["items"]["registryId"] = json!("urn:registry:models");
    let bytes = serde_json::to_vec(&root).unwrap();
    let imported = DocumentBytes::new(Path::new("models.json"), &models);
    let report = validate_registry(DocumentBytes::new(Path::new("api.json"), &bytes), [imported]);
    assert!(report.valid, "{report:#?}");

    for (field, empty) in [("imports", json!([])), ("dependencies", json!([]))] {
        let mut invalid = root.clone();
        if field == "imports" {
            invalid[field] = empty;
        } else {
            invalid["components"][0][field] = empty;
        }
        let bytes = serde_json::to_vec(&invalid).unwrap();
        let report = validate_registry(DocumentBytes::new(Path::new("api.json"), &bytes), [imported]);
        assert!(!report.valid, "missing {field} unexpectedly resolved: {report:#?}");
    }

    let mut tampered = models;
    tampered.push(b' ');
    let report = validate_registry(
        DocumentBytes::new(Path::new("api.json"), &bytes),
        [DocumentBytes::new(Path::new("models.json"), &tampered)],
    );
    assert!(!report.valid, "changed import bytes unexpectedly resolved");
    assert!(report.issues.iter().any(|issue| issue.message.contains("digest")), "{report:#?}");
}
