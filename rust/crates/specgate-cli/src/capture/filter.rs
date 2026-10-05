//! Component selection and top-level capture filtering.

use super::facade::BundleRequest;
use super::{Capture, ComponentId, ContextError, Registry};

/// Select the scenarios that belong to `component`, by *top-level* operation
/// only.
///
/// Each scenario contributes the subtrees rooted at its top-level operations of
/// `component`, verbatim: nested operations from other annotated components
/// stay in the trace with their original `parent_span_id`. A foreign top-level
/// operation and its whole subtree are dropped, and a scenario with no
/// top-level operation of `component` contributes nothing.
pub(super) fn filter(scenarios: impl AsRef<[Capture]>, component: &ComponentId) -> Vec<Capture> {
    let scenarios = scenarios.as_ref();
    let mut captures = Vec::with_capacity(scenarios.len());
    let retained_capacity = scenarios.iter().map(|capture| capture.operations.len()).max().unwrap_or_default();
    let mut retained = rustc_hash::FxHashSet::with_capacity_and_hasher(retained_capacity, rustc_hash::FxBuildHasher);
    for capture in scenarios {
        retained.clear();
        retained.extend(
            capture
                .operations
                .iter()
                .filter(|operation| {
                    operation.parent_span_id == capture.scenario.span_id && operation.component_id.as_str() == component.as_str()
                })
                .map(|operation| operation.span_id.clone()),
        );
        if retained.is_empty() {
            continue;
        }
        // Capture records parents before children, so adding each child as its
        // parent becomes retained computes the transitive subtree in one pass.
        for operation in &capture.operations {
            if retained.contains(&operation.parent_span_id) {
                retained.insert(operation.span_id.clone());
            }
        }
        let mut filtered = capture.clone();
        filtered.operations.retain(|operation| retained.contains(&operation.span_id));
        filtered.operations.shrink_to_fit();
        captures.push(filtered);
    }
    captures.shrink_to_fit();
    captures
}

pub(super) fn select_component(registry: &Registry, component: impl AsRef<str>) -> Result<ComponentId, ContextError> {
    let component = component.as_ref();
    let components = registry.present_components();
    if component.is_empty() {
        return match components.len() {
            0 => Err(ContextError::domain(
                "no components found: target has no annotated operations or types",
            )),
            1 => Ok(ComponentId::from(components[0].as_str())),
            _ => Err(ContextError::domain(format!(
                "multiple components present ({}); select one with --component <id>",
                components
                    .iter()
                    .map(specgate_discovery::identity::ComponentId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        };
    }
    if components.iter().any(|candidate| candidate == component) {
        Ok(ComponentId::from(component))
    } else {
        Err(ContextError::domain(format!(
            "component '{component}' not found; available components: {}",
            components.join(", ")
        )))
    }
}

/// Reject a capture batch that selects a component with an async setup.
///
/// An async `#[spec_operation]` rejects native capture from inside its own
/// body, but an async `#[spec_setup]` is deliberately left uninstrumented:
/// capture state is thread-local and cannot follow a future across executor
/// threads. Capturing such a component would therefore succeed while silently
/// dropping the setup's construction inputs, encoding a bundle that misstates
/// the component's public input surface. The whole component is rejected here
/// instead — before any test binary is built, run, or encoded — so the failure
/// names the setup rather than surfacing as a missing input much later.
pub(super) fn validate_setups(registry: &Registry, requests: impl AsRef<[BundleRequest]>) -> Result<(), ContextError> {
    for request in requests.as_ref() {
        let component = request.component.as_str();
        let setup = registry
            .ops
            .iter()
            .filter(|candidate| candidate.is_setup && candidate.is_async && candidate.component == component)
            .filter(|candidate| !excluded(request, &candidate.name))
            .min_by(|left, right| left.name.cmp(&right.name).then_with(|| left.fn_name.cmp(&right.fn_name)));
        if let Some(setup) = setup {
            return Err(ContextError::domain(format!(
                "component '{component}' declares async setup '{}' for operation '{component}::{}'; native capture cannot instrument an async setup, so this component is discovery-only until capture context is task-safe",
                setup.fn_name, setup.name
            )));
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "test-util"))]
pub(super) fn excluded(request: &BundleRequest, operation: impl AsRef<str>) -> bool {
    request.excluded_operations.contains(operation.as_ref())
}

#[cfg(not(any(test, feature = "test-util")))]
pub(super) fn excluded(_request: &BundleRequest, _operation: impl AsRef<str>) -> bool {
    false
}
