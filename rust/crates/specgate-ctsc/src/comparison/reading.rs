//! Diagnostic-only readings of validated values. Never used to decide equality.

use crate::validation::{AnyValue, RegistrySet, ResolvedComponent, TypeRef, canonical_value};
use crate::value_path::literal_path;

pub(super) type Readings = Vec<(String, String)>;

pub(super) fn optional_readings(
    value: &AnyValue,
    value_type: &TypeRef,
    component: &ResolvedComponent,
    registry: &RegistrySet,
    path: &str,
) -> Option<Readings> {
    let mut reader = Reader {
        registry,
        readings: Vec::new(),
        saw_optional: false,
    };
    reader.visit(value, value_type, component, path)?;
    reader.saw_optional.then_some(reader.readings)
}

pub(super) fn display(readings: &Readings) -> String {
    readings
        .iter()
        .map(|(path, value)| format!("{path}: {value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

struct Reader<'a> {
    registry: &'a RegistrySet,
    // Occurrences stay ordered, even if display paths coincide.
    readings: Readings,
    saw_optional: bool,
}

impl Reader<'_> {
    fn visit(&mut self, value: &AnyValue, ty: &TypeRef, component: &ResolvedComponent, path: &str) -> Option<()> {
        match ty {
            TypeRef::Named {
                name,
                component_id,
                registry_id,
            } => {
                let target = self
                    .registry
                    .components
                    .get(component_id.as_deref().unwrap_or(&component.component.id))?;
                if registry_id.as_ref().is_some_and(|id| id != &target.registry_id) {
                    return None;
                }
                let definition = target.component.types.iter().find(|item| item.name() == name)?;
                self.visit(value, &definition.as_type(), target, path)?;
            }
            TypeRef::Optional { value: inner } => {
                self.saw_optional = true;
                let AnyValue::KvList(entries) = value else { return None };
                let (tag, payload) = entries.first_key_value()?;
                match tag.as_str() {
                    "None" => self.readings.push((path.to_string(), "none".to_string())),
                    "Some" => {
                        let path = if matches!(inner.as_ref(), TypeRef::Optional { .. }) {
                            format!("{path}.some")
                        } else {
                            path.to_string()
                        };
                        self.visit(payload, inner, component, &path)?;
                    }
                    _ => return None,
                }
            }
            TypeRef::Record { fields } => {
                let AnyValue::KvList(entries) = value else { return None };
                if fields.is_empty() {
                    self.readings.push((path.to_string(), "()".to_string()));
                }
                for field in fields {
                    self.visit(
                        entries.get(field.name.as_str())?,
                        &field.value_type,
                        component,
                        &literal_path(path, &field.name),
                    )?;
                }
            }
            TypeRef::Tuple { items } => {
                let AnyValue::Array(values) = value else { return None };
                if items.is_empty() {
                    self.readings.push((path.to_string(), "()".to_string()));
                }
                for (index, (value, ty)) in values.iter().zip(items).enumerate() {
                    self.visit(value, ty, component, &format!("{path}[{index}]"))?;
                }
            }
            TypeRef::TaggedUnion { variants } => {
                let AnyValue::KvList(entries) = value else { return None };
                let (tag, payload) = entries.first_key_value()?;
                let variant = variants.iter().find(|variant| variant.name == *tag)?;
                let path = literal_path(path, tag);
                if let Some(ty) = &variant.payload {
                    self.visit(payload, ty, component, &path)?;
                } else {
                    self.readings.push((path, "()".to_string()));
                }
            }
            TypeRef::Primitive { name } if name == "unit" => {
                self.readings.push((path.to_string(), "()".to_string()));
            }
            // Containers remain atomic: canonical rendering retains every occurrence
            // and its existing laws without inventing element pairings.
            TypeRef::Primitive { .. } | TypeRef::Collection { .. } | TypeRef::Map { .. } => {
                let value = canonical_value(value, ty, component, self.registry)?;
                self.readings.push((path.to_string(), value.display()));
            }
        }
        Some(())
    }
}
