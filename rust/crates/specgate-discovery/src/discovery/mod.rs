//! Discovery workflows normalize native metadata into CTSC semantic schemas.
//!
//! # Example
//! ```no_run
//! use specgate_discovery::discover;
//! let component = specgate_discovery::identity::ComponentId::from("demo");
//! let discovered = discover("specgate.yaml", &component, None)?;
//! assert!(!discovered.registry_json.is_empty());
//! # Ok::<(), specgate_discovery::Error>(())
//! ```

//! CTSC implementation metadata discovery and semantic normalization.
//!
//! Rust implementations self-report link-time operation/type metadata. C#
//! implementations are built normally and reflected from their compiled
//! assembly. Both retain raw native invocation metadata and normalize the same
//! semantic schema, including deterministic setup folding.

use crate::error::{Error, ErrorKind};
use crate::identity::ComponentId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn failure(kind: ErrorKind, message: impl Into<String>) -> Error {
    Error::message(kind, message)
}

/// Discover raw and normalized metadata for one component implementation.
///
/// The implementation is selected from the binding by `component` and the
/// optional `implementation` label (omitted selects a sole implementation).
/// Rust implementations use link-time registration and C# implementations use
/// reflection over the compiled assembly. Discovery fails when the selected
/// sources do not declare `component`.
///
/// # Errors
///
/// Returns binding, selection, build, reflection, registry, normalization,
/// declared-component, or setup-ambiguity errors.
///
/// With the `test-util` feature, [`crate::test_util::discover_fake`] exercises
/// this complete load, resolve, and discovery workflow through concrete fake
/// binding and process/filesystem adapters.
///
/// # Panics
///
/// Panics if successful single-component discovery violates its non-empty
/// registry invariant.
pub fn discover(binding_path: impl AsRef<Path>, component: &ComponentId, implementation: Option<&str>) -> Result<Discovered, Error> {
    let resolved = crate::binding::resolve_implementation(binding_path, component.as_str(), implementation)?;
    discover_resolved(resolved)
}

/// Discover raw and normalized metadata from an already resolved implementation.
///
/// # Errors
///
/// Returns build, reflection, registry, normalization, declared-component, or
/// setup errors.
///
/// # Panics
///
/// Panics if successful single-component discovery violates its non-empty
/// registry invariant.
pub fn discover_resolved(implementation: crate::binding::ResolvedImplementation) -> Result<Discovered, Error> {
    let component = implementation.component.clone();
    discover_many(implementation, std::slice::from_ref(&component))?.into_selected()
}

/// Discover every component of a C# implementation from an assembly that is
/// already built, without building it again.
///
/// `artifacts` is the `--artifacts-path` of a build that included the sources
/// project: its output is located exactly as discovery's own build locates it,
/// and a single reflection pass reports every component the assembly declares.
/// A capture whose tests build already compiles (and weaves) the sources uses
/// this to learn the metadata of the bound component and of every component
/// nested inside its captures without a second compilation.
///
/// # Errors
///
/// Returns an error for a non-C# implementation, a missing assembly, or a
/// reflection or registry parse failure.
pub fn discover_built(implementation: crate::binding::ResolvedImplementation, artifacts: impl AsRef<Path>) -> Result<Batch, Error> {
    discover_built_in(implementation, artifacts.as_ref(), &crate::runner::system::System::real())
}

pub(crate) fn discover_built_in(
    implementation: crate::binding::ResolvedImplementation,
    artifacts: &Path,
    system: &crate::runner::system::System,
) -> Result<Batch, Error> {
    if implementation.language() != crate::binding::Language::CSharp {
        return Err(failure(
            ErrorKind::CSharp,
            "discovery from a prebuilt assembly is only available for C# implementations",
        ));
    }
    let reflected = crate::csharp_discovery::reflect_built(&implementation.implementation, artifacts, system)?;
    let names = reflected.present_components.clone();
    let requested = names.iter().map(String::as_str).collect::<Vec<_>>();
    assemble(
        implementation,
        &requested,
        Assembled {
            registry_json: reflected.documents,
            cargo_context: None,
            shared_document: false,
            reported_components: Some(reflected.present_components),
        },
    )
}

/// Fail clearly when the selected sources do not declare the bound component.
///
/// # Errors
///
/// Returns a binding error naming the implementation and the components the
/// sources do declare.
pub(crate) fn require_declared(implementation: &crate::binding::ResolvedImplementation, declared: &[ComponentId]) -> Result<(), Error> {
    if declared.contains(&implementation.component) {
        return Ok(());
    }
    Err(failure(
        ErrorKind::Binding,
        format!(
            "implementation '{}' of component '{}' does not declare that component in sources '{}'; declared components: {}",
            implementation.label,
            implementation.component,
            implementation.implementation.sources.display(),
            declared.iter().map(ComponentId::as_str).collect::<Vec<_>>().join(", ")
        ),
    ))
}

/// Discover raw and normalized metadata for many components of one implementation.
///
/// The implementation is selected by `component` and the optional label, and
/// then serves every component in `components`.
///
/// # Errors
///
/// Returns binding, selection, build, reflection, or registry parse errors. A
/// component whose metadata cannot be normalized is reported in its own
/// [`Component::schema`] rather than failing the whole batch.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::{discover_batch, identity::ComponentId, output::SchemaLookup};
/// let selected = [ComponentId::from("example.orders"), ComponentId::from("example.users")];
/// let batch = discover_batch("specgate.yaml", &selected[0], None, &selected)?;
/// match batch.schema("example.orders") {
///     SchemaLookup::Found(schema) => assert_eq!(schema.component.as_str(), "example.orders"),
///     SchemaLookup::Invalid(error) => eprintln!("orders metadata is invalid: {error}"),
///     SchemaLookup::Missing => eprintln!("orders was not published by the implementation"),
/// }
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
pub fn discover_batch(
    binding_path: impl AsRef<Path>,
    component: &ComponentId,
    implementation: Option<&str>,
    components: impl AsRef<[ComponentId]>,
) -> Result<Batch, Error> {
    let resolved = crate::binding::resolve_implementation(binding_path, component.as_str(), implementation)?;
    discover_many(resolved, components)
}

/// Discover metadata for many components of one resolved implementation.
///
/// The implementation pays the build and reflection cost once. Unlike
/// [`discover_resolved`], a requested component the sources do not declare is
/// reported through [`Batch::present_components`] and [`Batch::schema`] rather
/// than as an error.
///
/// # Errors
///
/// Returns build, reflection, or registry parse errors. C# discovery also
/// returns an error when no non-empty component is requested.
///
/// # Testing
///
/// With the `test-util` feature enabled, use
/// [`crate::test_util::discover_with`] and a
/// [`crate::test_util::Discovery`] to inject process, filesystem, and
/// environment outcomes without performing real system calls.
pub fn discover_many(
    implementation: crate::binding::ResolvedImplementation,
    components: impl AsRef<[ComponentId]>,
) -> Result<Batch, Error> {
    discover_in(implementation, components.as_ref(), &crate::runner::system::System::real())
}

pub(crate) fn discover_in(
    implementation: crate::binding::ResolvedImplementation,
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
    let language = implementation.language();
    let produced = match language {
        crate::binding::Language::Rust => {
            let context = crate::runner::candidate_in(&implementation.implementation.sources, system)?;
            Assembled {
                registry_json: vec![runner::run_in(&context, system)?],
                cargo_context: Some(context),
                shared_document: true,
                reported_components: None,
            }
        }
        crate::binding::Language::CSharp => {
            if requested.is_empty() {
                return Err(failure(ErrorKind::CSharp, "C# discovery requires a non-empty component"));
            }
            let (first, rest) = requested.split_first().expect("requested components were checked as non-empty");
            let discovered = crate::csharp_discovery::run_many(&implementation.implementation, first, rest, system)?;
            Assembled {
                registry_json: discovered.documents,
                cargo_context: None,
                shared_document: false,
                reported_components: Some(discovered.present_components),
            }
        }
    };
    assemble(implementation, &requested, produced)
}

/// What a discovery run produced, before it is parsed and normalized.
struct Assembled {
    registry_json: Vec<String>,
    cargo_context: Option<crate::runner::CandidatePackage>,
    shared_document: bool,
    reported_components: Option<Vec<String>>,
}

/// Parse and normalize what a discovery run produced into a [`Batch`].
fn assemble(implementation: crate::binding::ResolvedImplementation, requested: &[&str], produced: Assembled) -> Result<Batch, Error> {
    let Assembled {
        registry_json,
        cargo_context,
        shared_document,
        reported_components,
    } = produced;
    let language = implementation.language();
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
            let schema = normalize_registry(registry, language, *component);
            Ok((ComponentId::from(*component), Component { registry_index, schema }))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    Ok(Batch {
        implementation,
        cargo_context,
        registry_json,
        registries,
        components: discovered,
        present_components,
    })
}

/// Discover only the raw registry JSON for one component implementation.
///
/// # Errors
///
/// Returns the same errors as [`discover`].
///
/// # Panics
///
/// Panics if successful single-component discovery violates its non-empty
/// registry invariant.
pub fn registry_json(binding_path: impl AsRef<Path>, component: &ComponentId, implementation: Option<&str>) -> Result<String, Error> {
    Ok(discover(binding_path, component, implementation)?.registry_json)
}

/// Discover one component's normalized, setup-folded schema.
///
/// # Errors
///
/// Returns the same errors as [`discover`].
///
/// # Panics
///
/// Panics if successful single-component discovery violates its non-empty
/// registry invariant.
pub fn schema(binding_path: impl AsRef<Path>, component: &ComponentId, implementation: Option<&str>) -> Result<Schema, Error> {
    Ok(discover(binding_path, component, implementation)?.schema)
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
/// let discovered = discover("specgate.yaml", &component, None)?;
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
    reject_ambiguous_variants(&schema)?;
    Ok(schema)
}

pub(crate) mod model;
use model::{Batch, Component, Discovered, Schema};
mod normalize;
use normalize::{build_normalized, build_schema, reject_ambiguous_variants, reject_dynamic, validate_surface};
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
