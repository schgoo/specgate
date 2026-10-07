//! Normalized discovery-schema model, dependency validation, and CTSC conversion.

use super::{
    BTreeMap, BTreeSet, Deserialize, ErrorOutcome as CtscError, NamedType, NamedValue, Operation as CtscOperation, Outcomes,
    RegistryVariant, SETUP_EXTENSION, Serialize, TypeRef, TypeShape,
};
use crate::replay::model::{ComponentId, TypeOwner};
pub(super) mod error;

/// Component identity accepted at the discovery-schema boundary.
///
/// This semantic wrapper preserves every component spelling accepted by the
/// discovery-schema wire boundary.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub(super) struct ComponentName(String);

impl ComponentName {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ComponentName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::ops::Deref for ComponentName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for ComponentName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for ComponentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Unparsed type expression accepted at the discovery-schema boundary.
///
/// Parsing and reference validation happen when the normalized model is
/// converted, while this wrapper prevents confusion with ordinary names.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub(super) struct TypeExpr(String);

impl TypeExpr {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for TypeExpr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Deserialize)]
pub(super) struct Schema {
    pub(super) component: ComponentName,
    #[serde(default)]
    pub(super) dependencies: Vec<ComponentName>,
    #[serde(default)]
    pub(super) dependency_types: Vec<DependencyTypes>,
    pub(super) operations: Vec<Operation>,
    pub(super) types: Vec<Type>,
}

#[derive(Deserialize)]
pub(super) struct DependencyTypes {
    pub(super) component: ComponentName,
    #[serde(default)]
    pub(super) dependencies: Vec<ComponentName>,
    pub(super) types: Vec<Type>,
}

pub(super) struct TypeContext<'a> {
    pub(super) component: &'a str,
    pub(super) dependencies: &'a BTreeSet<ComponentName>,
    pub(super) types_by_component: &'a BTreeMap<String, BTreeSet<String>>,
}

pub(super) fn insert_names(
    types_by_component: &mut BTreeMap<String, BTreeSet<String>>,
    component: impl AsRef<str>,
    types: impl AsRef<[Type]>,
) -> error::Result<()> {
    let component = component.as_ref();
    let types = types.as_ref();
    if types_by_component.contains_key(component) {
        return Err(format!("normalized schema repeats component type owner '{component}'").into());
    }
    let names = types.iter().map(|ty| ty.name.clone()).collect::<BTreeSet<_>>();
    if names.len() != types.len() {
        return Err(format!("normalized component '{component}' contains duplicate type names").into());
    }
    types_by_component.insert(component.to_string(), names);
    Ok(())
}

pub(super) fn validate_cycles(schema: &Schema) -> error::Result<()> {
    let mut graph = schema
        .dependency_types
        .iter()
        .map(|dependency| (dependency.component.as_ref(), dependency.dependencies.as_slice()))
        .collect::<BTreeMap<_, _>>();
    graph.insert(schema.component.as_ref(), schema.dependencies.as_slice());
    let mut complete = BTreeSet::new();
    let mut visiting = Vec::with_capacity(graph.len());
    visit_dependency(&schema.component, &graph, &mut complete, &mut visiting)?;
    if complete.len() != graph.len() {
        let unreachable = graph
            .keys()
            .filter(|component| !complete.contains(**component))
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("normalized schema contains unreachable dependency components: {unreachable}").into());
    }
    Ok(())
}

pub(super) fn visit_dependency<'a>(
    component: &'a str,
    graph: &BTreeMap<&'a str, &'a [ComponentName]>,
    complete: &mut BTreeSet<&'a str>,
    visiting: &mut Vec<&'a str>,
) -> error::Result<()> {
    if complete.contains(component) {
        return Ok(());
    }
    if let Some(position) = visiting.iter().position(|candidate| *candidate == component) {
        let mut cycle = visiting[position..].to_vec();
        cycle.push(component);
        return Err(format!("component dependency cycle: {}", cycle.join(" -> ")).into());
    }
    visiting.push(component);
    for dependency in graph.get(component).copied().unwrap_or_default() {
        visit_dependency(dependency.as_ref(), graph, complete, visiting)?;
    }
    visiting.pop();
    complete.insert(component);
    Ok(())
}

#[derive(Deserialize)]
pub(super) struct Operation {
    pub(super) name: String,
    #[serde(rename = "is_async")]
    pub(super) _is_async: bool,
    pub(super) inputs: Vec<Input>,
    pub(super) output: TypeExpr,
    #[serde(default)]
    pub(super) empty: bool,
    #[serde(default)]
    pub(super) errors: Vec<Error>,
    #[serde(default)]
    pub(super) setups: Vec<Setup>,
}

impl Operation {
    pub(super) fn to_ctsc(&self, context: &TypeContext<'_>) -> error::Result<CtscOperation> {
        let inputs = self
            .inputs
            .iter()
            .map(|input| {
                Ok(NamedValue {
                    name: input.name.clone(),
                    value_type: type_ref(&input.ty, context)?,
                })
            })
            .collect::<error::Result<Vec<_>>>()?;
        let result = if is_unit(&self.output) {
            None
        } else {
            Some(type_ref(&self.output, context)?)
        };
        let errors = self
            .errors
            .iter()
            .map(|error| {
                Ok(CtscError {
                    name: error.name.clone(),
                    value_type: if is_unit(&error.ty) {
                        None
                    } else {
                        Some(type_ref(&error.ty, context)?)
                    },
                })
            })
            .collect::<error::Result<Vec<_>>>()?;

        Ok(CtscOperation {
            name: self.name.clone(),
            inputs,
            observations: Vec::new(),
            outcomes: Outcomes {
                result,
                empty: self.empty.then_some(true),
                errors,
            },
            extensions: (!self.setups.is_empty()).then(|| {
                BTreeMap::from([(
                    SETUP_EXTENSION.to_string(),
                    serde_json::to_value(&self.setups).expect("normalized setups serialize"),
                )])
            }),
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Error {
    pub(super) name: String,
    pub(super) ty: TypeExpr,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Setup {
    pub(super) fills: String,
    pub(super) inputs: Vec<Input>,
    pub(super) output: TypeExpr,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Input {
    pub(super) name: String,
    pub(super) ty: TypeExpr,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum TypeKind {
    Struct,
    Enum,
}

#[derive(Deserialize)]
pub(super) struct Type {
    pub(super) name: String,
    pub(super) kind: TypeKind,
    #[serde(default)]
    pub(super) fields: Vec<Field>,
    #[serde(default)]
    pub(super) variants: Vec<Variant>,
}

impl Type {
    pub(super) fn to_ctsc(&self, context: &TypeContext<'_>) -> error::Result<NamedType> {
        let shape = match self.kind {
            TypeKind::Struct => {
                if !self.variants.is_empty() {
                    return Err(format!("struct type '{}' must not declare variants", self.name).into());
                }
                TypeShape::Record {
                    fields: normalized_fields(&self.fields, context)?,
                }
            }
            TypeKind::Enum => {
                if !self.fields.is_empty() {
                    return Err(format!("enum type '{}' must not declare fields", self.name).into());
                }
                if self.variants.is_empty() {
                    return Err(format!("enum type '{}' must declare at least one variant", self.name).into());
                }
                TypeShape::TaggedUnion {
                    variants: self
                        .variants
                        .iter()
                        .map(|variant| {
                            if variant.tuple.is_some() && !variant.fields.is_empty() {
                                return Err(format!(
                                    "enum type '{}' variant '{}' must not declare both named and tuple payloads",
                                    self.name, variant.name
                                )
                                .into());
                            }
                            let payload = if let Some(tuple) = &variant.tuple {
                                Some(TypeRef::tuple(
                                    tuple
                                        .iter()
                                        .map(|field_type| type_ref(field_type, context))
                                        .collect::<Result<Vec<_>, _>>()?,
                                ))
                            } else if variant.fields.is_empty() {
                                None
                            } else {
                                Some(TypeRef::record(normalized_fields(&variant.fields, context)?))
                            };
                            Ok(RegistryVariant {
                                name: variant.name.clone(),
                                payload,
                            })
                        })
                        .collect::<error::Result<Vec<_>>>()?,
                }
            }
        };

        Ok(NamedType {
            name: self.name.clone(),
            shape,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Field {
    pub(super) name: String,
    pub(super) ty: TypeExpr,
}

#[derive(Deserialize)]
pub(super) struct Variant {
    pub(super) name: String,
    #[serde(default)]
    pub(super) fields: Vec<Field>,
    pub(super) tuple: Option<Vec<TypeExpr>>,
}

pub(super) fn normalized_fields(fields: impl AsRef<[Field]>, context: &TypeContext<'_>) -> error::Result<Vec<NamedValue>> {
    fields
        .as_ref()
        .iter()
        .map(|field| {
            Ok(NamedValue {
                name: field.name.clone(),
                value_type: type_ref(&field.ty, context)?,
            })
        })
        .collect()
}

pub(super) fn type_ref(type_ref: impl AsRef<str>, context: &TypeContext<'_>) -> error::Result<TypeRef> {
    let type_ref = type_ref.as_ref();
    let mut parser = TypeParser {
        input: type_ref,
        position: 0,
        context,
    };
    let parsed = parser.parse_type()?;
    parser.skip_whitespace();
    if parser.position != parser.input.len() {
        return Err(format!(
            "unexpected trailing input '{}' in type reference '{type_ref}'",
            &parser.input[parser.position..]
        )
        .into());
    }
    Ok(parsed)
}

pub(super) struct TypeParser<'a> {
    pub(super) input: &'a str,
    pub(super) position: usize,
    pub(super) context: &'a TypeContext<'a>,
}

impl TypeParser<'_> {
    pub(super) fn parse_type(&mut self) -> error::Result<TypeRef> {
        self.skip_whitespace();
        if self.remaining().starts_with("()") {
            self.position += 2;
            return Ok(TypeRef::primitive("unit"));
        }

        let name = self.parse_identifier()?;
        self.skip_whitespace();
        if self.consume('<') {
            let arguments = self.parse_arguments()?;
            return construct_generic(&name, arguments);
        }

        if is_primitive(&name) {
            Ok(TypeRef::primitive(name))
        } else {
            self.named_type(name)
        }
    }

    pub(super) fn named_type(&self, qualified_name: String) -> error::Result<TypeRef> {
        if let Some((component, name)) = qualified_name.rsplit_once("::") {
            if component != self.context.component && !self.context.dependencies.contains(component) {
                return Err(format!("named type '{qualified_name}' references undeclared component dependency '{component}'").into());
            }
            let known = self
                .context
                .types_by_component
                .get(component)
                .is_some_and(|types| types.contains(name));
            if !known {
                return Err(format!("unknown named type '{qualified_name}'").into());
            }
            return Ok(TypeRef::named(
                name.to_string(),
                TypeOwner::from_ids(
                    if component == self.context.component {
                        None
                    } else {
                        Some(ComponentId::try_new(component).map_err(|error| {
                            format!("invalid component identity '{component}' in type reference '{qualified_name}': {error}")
                        })?)
                    },
                    None,
                ),
            ));
        }
        let known = self
            .context
            .types_by_component
            .get(self.context.component)
            .is_some_and(|types| types.contains(&qualified_name));
        if known {
            Ok(TypeRef::named(qualified_name, TypeOwner::default()))
        } else {
            Err(format!("unknown named type '{qualified_name}'").into())
        }
    }

    pub(super) fn parse_identifier(&mut self) -> error::Result<String> {
        self.skip_whitespace();
        let start = self.position;
        while let Some(character) = self.remaining().chars().next() {
            if character.is_alphanumeric() || matches!(character, '_' | ':' | '.') {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
        if self.position == start {
            Err(format!("expected type name at byte {}", self.position).into())
        } else {
            Ok(self.input[start..self.position].to_string())
        }
    }

    pub(super) fn parse_arguments(&mut self) -> error::Result<Vec<TypeRef>> {
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.consume('>') {
            return Ok(arguments);
        }

        loop {
            arguments.push(self.parse_type()?);
            self.skip_whitespace();
            if self.consume('>') {
                return Ok(arguments);
            }
            if self.consume(',') {
                continue;
            }
            if self.position == self.input.len() {
                return Err(format!("expected '>' at byte {}", self.position).into());
            }
            return Err(format!("expected ',' or '>' at byte {}", self.position).into());
        }
    }

    pub(super) fn consume(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.remaining().starts_with(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    pub(super) fn skip_whitespace(&mut self) {
        while let Some(character) = self.remaining().chars().next() {
            if character.is_whitespace() {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
    }

    pub(super) fn remaining(&self) -> &str {
        &self.input[self.position..]
    }
}

pub(super) fn construct_generic(name: impl AsRef<str>, mut arguments: Vec<TypeRef>) -> error::Result<TypeRef> {
    let name = name.as_ref();
    match name {
        "List" | "list" => {
            expect_arity(name, &arguments, 1)?;
            Ok(TypeRef::list(arguments.remove(0)))
        }
        "Set" | "set" => {
            expect_arity(name, &arguments, 1)?;
            Ok(TypeRef::set(arguments.remove(0)))
        }
        "Map" | "map" => {
            expect_arity(name, &arguments, 2)?;
            let values = arguments.remove(1);
            let keys = arguments.remove(0);
            Ok(TypeRef::map(keys, values))
        }
        "Tuple" | "tuple" => {
            if arguments.is_empty() {
                return Err(format!("type constructor '{name}' expects at least 1 type argument").into());
            }
            arguments.shrink_to_fit();
            Ok(TypeRef::tuple(arguments))
        }
        "Option" | "optional" => {
            expect_arity(name, &arguments, 1)?;
            Ok(TypeRef::optional(arguments.remove(0)))
        }
        other => Err(format!("unsupported type constructor '{other}'").into()),
    }
}

pub(super) fn expect_arity(name: impl AsRef<str>, arguments: impl AsRef<[TypeRef]>, expected: usize) -> error::Result<()> {
    let name = name.as_ref();
    let arguments = arguments.as_ref();
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(format!("type constructor '{name}' expects {expected} type arguments").into())
    }
}

pub(super) fn is_primitive(name: impl AsRef<str>) -> bool {
    matches!(
        name.as_ref(),
        "unit" | "string" | "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" | "bytes"
    )
}

pub(super) fn is_unit(type_ref: impl AsRef<str>) -> bool {
    matches!(type_ref.as_ref().trim(), "" | "()" | "unit")
}
