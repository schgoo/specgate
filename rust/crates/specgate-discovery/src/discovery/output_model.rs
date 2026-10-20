//! Discovery outputs that pair normalized schemas with retained producer evidence.

use super::Schema;
use crate::discovery::registry::Registry;
use crate::error::Error;
use crate::identity::ComponentId;
use std::collections::BTreeMap;

/// Raw and normalized metadata for one selected component implementation.
#[derive(Debug, Clone)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery result DTOs expose their complete evidence to workspace consumers"
)]
pub struct Discovered {
    /// Resolved binding implementation.
    pub implementation: crate::binding::ResolvedImplementation,
    /// Candidate Cargo identity for Rust implementations.
    pub cargo_context: Option<crate::runner::CandidatePackage>,
    /// Raw registry JSON emitted by the implementation.
    pub registry_json: String,
    /// Parsed raw registry. This producer-interoperability surface is available
    /// only with the `raw-registry` feature.
    #[cfg(feature = "raw-registry")]
    pub registry: Registry,
    #[cfg(not(feature = "raw-registry"))]
    pub(crate) registry: Registry,
    /// Normalized selected component schema.
    pub schema: Schema,
}

/// Raw registry selection and normalized schema for one requested component.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery result DTOs expose their complete evidence to workspace consumers"
)]
pub struct Component {
    /// Index of this component's document in [`Batch::registries`].
    pub registry_index: usize,
    /// Normalized schema, or this component's exact normalization failure.
    pub schema: Result<Schema, Error>,
}

/// Outcome of looking up one component in a discovery batch.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "schema lookup has a closed result taxonomy exhaustively handled by callers"
)]
pub enum SchemaLookup<'a> {
    /// The component was not requested.
    Missing,
    /// The component normalized successfully.
    Found(&'a Schema),
    /// The component was requested but normalization failed.
    Invalid(&'a Error),
}

/// Raw and normalized metadata for many components of one binding target.
///
/// Rust targets link and self-report once, so every component shares registry
/// index `0`. C# targets build and reflect once, emitting one raw document per
/// component. Either way, the expensive toolchain work happens a single time.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::output::{Batch, SchemaLookup};
///
/// # fn inspect(batch: &Batch) {
/// match batch.schema("example.math") {
///     SchemaLookup::Found(schema) => println!("{}", schema.component),
///     SchemaLookup::Missing => println!("component was not requested"),
///     SchemaLookup::Invalid(error) => eprintln!("{error}"),
/// }
///
/// if let Some(registry) = batch.registry("example.math") {
///     println!("{registry:?}");
/// }
/// if let Some(document) = batch.registry_json("example.math") {
///     println!("{document}");
/// }
/// # }
/// ```
#[derive(Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery result DTOs expose their complete evidence to workspace consumers"
)]
pub struct Batch {
    /// Resolved binding implementation.
    pub implementation: crate::binding::ResolvedImplementation,
    /// Candidate Cargo identity for Rust implementations.
    pub cargo_context: Option<crate::runner::CandidatePackage>,
    /// Raw registry documents in emission order.
    pub registry_json: Vec<String>,
    /// Parsed registries aligned with `registry_json`. This producer-
    /// interoperability surface is available only with `raw-registry`.
    #[cfg(feature = "raw-registry")]
    pub registries: Vec<Registry>,
    #[cfg(not(feature = "raw-registry"))]
    pub(crate) registries: Vec<Registry>,
    /// Requested components, sorted and deduplicated.
    pub components: BTreeMap<ComponentId, Component>,
    /// Every component the compiled target declares an operation for, sorted
    /// and deduplicated, independent of what this run requested. Callers that
    /// must account for the whole target compare their expected coverage
    /// against this rather than against the requested subset.
    pub present_components: Vec<ComponentId>,
}

impl Batch {
    /// Narrow the batch to the component the binding selected.
    ///
    /// The selected component's registry and normalized schema become a
    /// [`Discovered`]; the sources must declare that component.
    ///
    /// # Errors
    ///
    /// Returns a binding error when the sources do not declare the selected
    /// component, or the component's normalization failure.
    ///
    /// # Panics
    ///
    /// Panics if the batch lacks a registry document for the selected
    /// component, which successful discovery guarantees.
    pub fn into_selected(mut self) -> Result<Discovered, Error> {
        crate::discovery::require_declared(&self.implementation, &self.present_components)?;
        let component = self.implementation.component.clone();
        let selected = self
            .components
            .remove(&component)
            .expect("successful discovery must contain the selected component");
        let schema = selected.schema?;
        let index = selected.registry_index;
        let registry_json = self.registry_json.swap_remove(index);
        let registry = self.registries.swap_remove(index);
        Ok(Discovered {
            implementation: self.implementation,
            cargo_context: self.cargo_context,
            registry_json,
            registry,
            schema,
        })
    }

    /// The parsed registry backing `component`.
    ///
    /// # Panics
    ///
    /// Panics if internal component metadata references no backing registry.
    #[must_use]
    pub fn registry(&self, component: impl AsRef<str>) -> Option<&Registry> {
        let component = component.as_ref();
        self.components.get(component).map(|discovered| {
            let index = discovered.registry_index;
            let count = self.registries.len();
            self.registries
                .get(index)
                .unwrap_or_else(|| panic!("component registry index {index} must reference one of {count} registries"))
        })
    }

    /// The raw registry document backing `component`.
    ///
    /// # Panics
    /// Panics if internal component metadata references no backing registry document.
    #[must_use]
    pub fn registry_json(&self, component: impl AsRef<str>) -> Option<&str> {
        let component = component.as_ref();
        self.components.get(component).map(|discovered| {
            let index = discovered.registry_index;
            let count = self.registry_json.len();
            self.registry_json
                .get(index)
                .unwrap_or_else(|| panic!("component registry JSON index {index} must reference one of {count} documents"))
                .as_str()
        })
    }

    /// The normalized schema result for `component`.
    #[must_use]
    pub fn schema(&self, component: impl AsRef<str>) -> SchemaLookup<'_> {
        let component = component.as_ref();
        match self.components.get(component).map(|discovered| &discovered.schema) {
            None => SchemaLookup::Missing,
            Some(Ok(schema)) => SchemaLookup::Found(schema),
            Some(Err(error)) => SchemaLookup::Invalid(error),
        }
    }
}
