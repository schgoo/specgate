//! Discovery workflows normalize native metadata into CTSC semantic schemas.
//!
//! # Example
//! ```no_run
//! use specgate_discovery::discover;
//! let component = specgate_discovery::identity::ComponentId::from("demo");
//! let discovered = discover("specgate.yaml", None, &component)?;
//! assert!(!discovered.registry_json.is_empty());
//! # Ok::<(), specgate_discovery::Error>(())
//! ```

//! CTSC implementation metadata discovery and semantic normalization.
//!
//! Rust targets self-report link-time operation/type metadata. C# targets are
//! built normally and reflected from their compiled assembly. Both retain raw
//! native invocation metadata and normalize the same semantic schema, including
//! deterministic setup folding.

use crate::error::{Error, ErrorKind};
use crate::identity::ComponentId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn failure(kind: ErrorKind, message: impl Into<String>) -> Error {
    Error::message(kind, message)
}

/// Discover raw and normalized metadata for one target.
///
/// Rust targets use link-time registration and C# targets use reflection over
/// the compiled target assembly. Empty `component` is allowed for Rust callers
/// that need to select from the raw registry before normalization.
///
/// # Errors
///
/// Returns binding, target, build, reflection, registry, normalization, or
/// setup-ambiguity errors.
///
/// With the `test-util` feature, [`crate::test_util::discover_fake`] exercises
/// this complete load, resolve, and discovery workflow through concrete fake
/// binding and process/filesystem adapters.
///
/// # Panics
///
/// Panics if successful single-target discovery violates its non-empty
/// registry invariant.
pub fn discover(binding_path: impl AsRef<Path>, target_name: Option<&str>, component: &ComponentId) -> Result<Target, Error> {
    let target = crate::binding::resolve_target(binding_path, target_name)?;
    discover_resolved(target, component)
}

/// Discover raw and normalized metadata from an already resolved target.
///
/// # Errors
///
/// Returns build, reflection, registry, normalization, or setup errors.
///
/// # Panics
///
/// Panics if successful single-target discovery violates its non-empty
/// registry invariant.
pub fn discover_resolved(target: crate::binding::ResolvedTarget, component: &ComponentId) -> Result<Target, Error> {
    let component = component.as_str();
    let requested = if component.is_empty() {
        Vec::new()
    } else {
        vec![ComponentId::from(component)]
    };
    let mut many = discover_many(target, &requested)?;
    let schema = if component.is_empty() {
        Schema {
            component: ComponentId::default(),
            dependencies: Vec::new(),
            dependency_types: Vec::new(),
            operations: Vec::new(),
            types: Vec::new(),
        }
    } else {
        many.components
            .remove(component)
            .expect("successful discovery must contain every deduplicated requested component")
            .schema?
    };
    let registry_json = many
        .registry_json
        .into_iter()
        .next()
        .expect("successful single-component discovery contains a registry document");
    let registry = many
        .registries
        .into_iter()
        .next()
        .expect("successful single-component discovery contains a parsed registry");
    Ok(Target {
        target: many.target,
        cargo_context: many.cargo_context,
        registry_json,
        registry,
        schema,
    })
}

/// Discover raw and normalized metadata for many components of one target.
///
/// # Errors
///
/// Returns binding, target, build, reflection, or registry parse errors. A
/// component whose metadata cannot be normalized is reported in its own
/// [`Component::schema`] rather than failing the whole batch.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::{discover_batch, identity::ComponentId, output::SchemaLookup};
/// let selected = [ComponentId::from("example.orders"), ComponentId::from("example.users")];
/// let batch = discover_batch("specgate.yaml", None, &selected)?;
/// match batch.schema("example.orders") {
///     SchemaLookup::Found(schema) => assert_eq!(schema.component.as_str(), "example.orders"),
///     SchemaLookup::Invalid(error) => eprintln!("orders metadata is invalid: {error}"),
///     SchemaLookup::Missing => eprintln!("orders was not published by the target"),
/// }
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
pub fn discover_batch(
    binding_path: impl AsRef<Path>,
    target_name: Option<&str>,
    components: impl AsRef<[ComponentId]>,
) -> Result<Batch, Error> {
    let target = crate::binding::resolve_target(binding_path, target_name)?;
    discover_many(target, components)
}

/// Discover metadata for many components of one resolved target.
///
/// The implementation pays the build and reflection cost once.
///
/// # Errors
///
/// Returns build, reflection, or registry parse errors.
///
/// # Testing
///
/// With the `test-util` feature enabled, use
/// [`crate::test_util::discover_with`] and a
/// [`crate::test_util::Discovery`] to inject process, filesystem, and
/// environment outcomes without performing real system calls.
pub fn discover_many(target: crate::binding::ResolvedTarget, components: impl AsRef<[ComponentId]>) -> Result<Batch, Error> {
    discover_in(target, components.as_ref(), &crate::runner::system::System::real())
}

pub(crate) fn discover_in(
    target: crate::binding::ResolvedTarget,
    components: &[ComponentId],
    system: &crate::runner::system::System,
) -> Result<Batch, Error> {
    let mut requested = components
        .iter()
        .map(AsRef::as_ref)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    requested.sort_unstable();
    requested.dedup();
    let (registry_json, cargo_context, shared_document, reported_components) = match target.language {
        crate::binding::Language::Rust => {
            let context = crate::runner::candidate_in(&target.target.package_root, system)?;
            (vec![runner::run_in(&context, system)?], Some(context), true, None)
        }
        crate::binding::Language::CSharp => {
            if requested.is_empty() {
                return Err(failure(ErrorKind::CSharp, "C# discovery requires a non-empty component"));
            }
            let (first, rest) = requested.split_first().expect("requested components were checked as non-empty");
            let discovered = crate::csharp_discovery::run_many(&target.target, first, rest, system)?;
            (discovered.documents, None, false, Some(discovered.present_components))
        }
    };
    let registries = registry_json.iter().map(Registry::parse).collect::<Result<Vec<_>, Error>>()?;
    // A shared Rust document already describes the whole linked target; a C#
    // run reflects one document per request, so its inventory comes from the
    // same single reflection pass instead.
    let present_components = reported_components.map_or_else(
        || registries.first().map(Registry::present_components).unwrap_or_default(),
        |components| components.into_iter().map(ComponentId::from).collect(),
    );
    let discovered = requested
        .iter()
        .enumerate()
        .map(|(position, component)| {
            let registry_index = if shared_document { 0 } else { position };
            let registry = registries.get(registry_index).ok_or_else(|| {
                failure(
                    ErrorKind::Registry,
                    format!("discovery emitted no registry document for component '{component}'"),
                )
            })?;
            let schema = normalize_registry(registry, target.language, *component);
            Ok((ComponentId::from(*component), Component { registry_index, schema }))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    Ok(Batch {
        target,
        cargo_context,
        registry_json,
        registries,
        components: discovered,
        present_components,
    })
}

/// Discover only the raw registry JSON for one target.
///
/// # Errors
///
/// Returns the same errors as [`discover`].
///
/// # Panics
///
/// Panics if successful single-target discovery violates its non-empty
/// registry invariant.
pub fn registry_json(binding_path: impl AsRef<Path>, target_name: Option<&str>, component: &ComponentId) -> Result<String, Error> {
    Ok(discover(binding_path, target_name, component)?.registry_json)
}

/// Discover one target's normalized, setup-folded component schema.
///
/// # Errors
///
/// Returns the same errors as [`discover`].
///
/// # Panics
///
/// Panics if successful single-target discovery violates its non-empty
/// registry invariant.
pub fn schema(binding_path: impl AsRef<Path>, target_name: Option<&str>, component: &ComponentId) -> Result<Schema, Error> {
    Ok(discover(binding_path, target_name, component)?.schema)
}

/// Normalize a previously parsed raw registry for one language and component.
///
/// # Errors
///
/// Returns semantic identity, visibility, setup, or malformed metadata errors.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::{binding::Language, discover, identity::ComponentId, schema::normalize_registry};
/// let component = ComponentId::from("example.orders");
/// let discovered = discover("specgate.yaml", None, &component)?;
/// let normalized = normalize_registry(&discovered.registry, Language::Rust, component)?;
/// assert!(!normalized.operations.is_empty());
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
pub fn normalize_registry(
    registry: &Registry,
    language: crate::binding::Language,
    component: impl Into<ComponentId>,
) -> Result<Schema, Error> {
    let component = component.into();
    validate_surface(registry, &component)?;
    let schema = match language {
        crate::binding::Language::CSharp => build_normalized(registry, &component)?,
        crate::binding::Language::Rust => build_schema(registry, &component)?,
    };
    reject_dynamic(&schema)?;
    Ok(schema)
}

pub(crate) mod model;
use model::{Batch, Component, Schema, Target};
mod normalize;
use normalize::{build_normalized, build_schema, reject_dynamic, validate_surface};
pub(crate) mod registry;
use registry::Registry;
pub(crate) mod runner;
pub(crate) mod setup;
pub(crate) mod types;
#[cfg(test)]
use setup::raw_inputs;
#[cfg(test)]
use types::map;
#[cfg(test)]
use types::parse;
#[cfg(test)]
mod tests;
