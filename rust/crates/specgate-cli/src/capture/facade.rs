#![cfg_attr(all(feature = "test-util", not(test)), expect(dead_code, reason = "feature-gated fixture support"))]

//! Capture workflow orchestration and publication.
use super::*;

// Ordinary capture publishes evidence from failing tests; golden generation opts into rejecting them.
const REJECT_FAILURES: bool = false;

/// Capture passing tests for one Rust binding target as a native CTSC bundle.
///
/// An empty [`TargetName`] selects the binding default. An empty
/// [`ComponentId`] selects the sole discovered component and errors when
/// discovery is ambiguous.
///
/// # Errors
///
/// Returns an [`crate::CaptureError`] categorized by the stage that rejected the
/// request or failed to produce a complete bundle.
///
/// # Panics
///
/// Panics if the internal bundle writer violates its one-report-per-request invariant.
///
/// # Examples
///
/// ```no_run
/// use specgate_cli::{CapturePaths, CaptureRequest, capture};
///
/// let request = CaptureRequest::builder(CapturePaths {
///     binding: "binding.yaml".into(),
///     out: "capture".into(),
/// })
///     .target("rust")
///     .component("example.math")
///     .build()?;
/// match capture(request) {
///     Ok(report) => println!("{}", report.manifest_path.display()),
///     Err(error) if error.is_selection() => {
///         eprintln!("select a component: {}", error.diagnostic());
///     }
///     Err(error) => return Err(error.into()),
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
#[spec_operation("capture")]
pub fn capture(request: CaptureRequest) -> Result<CaptureReport, CaptureError> {
    capture_with(&request, &CommandEnvironment::real(), &Execution::real(), &Discovery::real())
}

/// Capture through caller-supplied filesystem, execution, and discovery services.
///
/// `system` owns filesystem, environment, and scratch behavior; `execution`
/// runs child processes; `discovery` resolves the binding metadata. Production
/// callers normally use [`capture`]. Tests enabling `test-util` can pass fake
/// services and queue deterministic results without touching the host.
///
/// # Examples
/// ```no_run
/// use specgate_cli::{capture_with, CapturePaths, CaptureRequest, Discovery, Execution, CommandEnvironment};
/// let request = CaptureRequest::builder(CapturePaths { binding: "binding.yaml".into(), out: "capture".into() }).build()?;
/// let report = capture_with(&request, &CommandEnvironment::real(), &Execution::real(), &Discovery::real())?;
/// assert!(report.manifest_path.ends_with("manifest.json"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
/// Returns a categorized capture failure from request execution or publication.
///
/// # Panics
/// Panics if the internal bundle writer violates its one-report-per-request invariant.
pub fn capture_with(
    request: &CaptureRequest,
    system: &CommandEnvironment,
    execution: &Execution,
    discovery: &Discovery,
) -> Result<CaptureReport, CaptureError> {
    let discovered =
        discover_target(request.binding_str(), request.target(), system, discovery).map_err(public_error(CaptureErrorKind::Discovery))?;
    let selected = select_component(&discovered.registry, request.component()).map_err(public_error(CaptureErrorKind::Selection))?;
    let requests = [BundleRequest {
        component: selected,
        out: request.out().to_path_buf(),
        #[cfg(any(test, feature = "test-util"))]
        excluded_operations: BTreeSet::new(),
    }];
    let executed = execute_with(&discovered, &requests, system, execution).map_err(public_error(CaptureErrorKind::Execution))?;
    let reports =
        write_with(&discovered, &requests, &executed, REJECT_FAILURES, system).map_err(public_error(CaptureErrorKind::Encoding))?;
    Ok(reports
        .into_iter()
        .next()
        .expect("one capture bundle request must produce one publication report"))
}

/// One requested component bundle within a batched capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleRequest {
    pub(crate) component: ComponentId,
    pub(crate) out: PathBuf,
    /// Golden-only operation exclusions. Product capture always leaves this
    /// empty; the golden matrix validates every declared exclusion separately.
    #[cfg(any(test, feature = "test-util"))]
    pub(crate) excluded_operations: BTreeSet<String>,
}

/// Capture many components from one Rust binding target in a single pass.
///
/// Discovery, the libtest build, test enumeration, and test execution each
/// happen exactly once for the whole batch; only bundle encoding is per
/// component. [`capture`] is the one-component case of this same path, so
/// batched and single bundles are byte-identical.
///
/// Callers that already hold a [`Target`] should use
/// [`capture_discovered`] directly and pay discovery once for the whole run.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_many(
    binding: impl AsRef<Path>,
    target: impl AsRef<str>,
    requests: &[BundleRequest],
) -> Result<Vec<CaptureReport>, ContextError> {
    assert!(!requests.is_empty(), "capture_many requires at least one bundle request");
    let discovered = discover(binding, target)?;
    let present = discovered.registry.present_components();
    for request in requests {
        if !present.iter().any(|candidate| candidate == request.component.as_str()) {
            return Err(ContextError::domain(format!(
                "component '{}' not found; available components: {}",
                request.component,
                present.join(", ")
            )));
        }
    }
    capture_discovered(&discovered, requests)
}

#[cfg(any(test, feature = "test-util"))]
pub(crate) fn discover(binding: impl AsRef<Path>, target: impl AsRef<str>) -> Result<Target, ContextError> {
    discover_target(binding, target, &CommandEnvironment::real(), &Discovery::real())
}

fn discover_target(
    binding: impl AsRef<Path>,
    target: impl AsRef<str>,
    system: &CommandEnvironment,
    discovery: &Discovery,
) -> Result<Target, ContextError> {
    let binding = binding.as_ref();
    let target = target.as_ref();
    let target_name = if target.is_empty() { None } else { Some(target) };
    let discovered = discovery
        .capture_target(binding, target_name, &specgate_discovery::identity::ComponentId::default())
        .map_err(|error| ContextError::with_source(error.to_string(), error))?;
    let resolved = &discovered.target;
    if resolved.language != specgate_discovery::binding::Language::Rust {
        return Err(ContextError::domain(format!(
            "capture currently supports only Rust targets; binding language is '{}'",
            resolved.language
        )));
    }
    if !system.is_file(resolved.target.package_root.join("Cargo.toml")) {
        return Err(ContextError::domain(format!(
            "capture target '{}' is not a Rust package (no Cargo.toml at {})",
            resolved.name,
            resolved.target.package_root.display()
        )));
    }
    Ok(discovered)
}

/// Capture many components, keeping the tests that pass.
///
/// This is `specgate capture`'s behavior: a failing test contributes no
/// scenario, and a component left with no scenario at all still errors.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_discovered(discovered: &Target, requests: &[BundleRequest]) -> Result<Vec<CaptureReport>, ContextError> {
    discovered_with(discovered, requests, FailureMode::Allow)
}

/// Capture many components, failing on any failing enumerated fixture test.
///
/// Used by the CTSC golden harness, where a fixture test that fails is a
/// product or fixture defect rather than a scenario to skip: the goldens claim
/// to be the corpus's real behavior, so a red test must not be hidden by a
/// sibling test that happens to cover the same component.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_strict(discovered: &Target, requests: &[BundleRequest]) -> Result<Vec<CaptureReport>, ContextError> {
    discovered_with(discovered, requests, FailureMode::Reject)
}

#[cfg(any(test, feature = "test-util"))]
#[derive(Clone, Copy)]
pub(super) enum FailureMode {
    Allow,
    Reject,
}

#[cfg(any(test, feature = "test-util"))]
pub(super) fn discovered_with(
    discovered: &Target,
    requests: &[BundleRequest],
    failure_mode: FailureMode,
) -> Result<Vec<CaptureReport>, ContextError> {
    let executed = execute_tests(discovered, requests)?;
    write_bundles(discovered, requests, &executed, matches!(failure_mode, FailureMode::Reject))
}

#[cfg(any(test, feature = "test-util"))]
pub(super) fn execute_tests(discovered: &Target, requests: &[BundleRequest]) -> Result<ExecutedTests, ContextError> {
    execute_with(discovered, requests, &CommandEnvironment::real(), &Execution::real())
}

fn execute_with(
    discovered: &Target,
    requests: &[BundleRequest],
    system: &CommandEnvironment,
    execution: &Execution,
) -> Result<ExecutedTests, ContextError> {
    let resolved = &discovered.target;
    validate_setups(&discovered.registry, requests)?;
    let scratch = scratch_dir(system, &resolved.target.package_root)?;
    let test_binaries = build_binaries(execution, &resolved.target.package_root, scratch.path())?;
    let tests = enumerate_tests(execution, &test_binaries)?;
    run_tests(system, execution, &tests, scratch.path())
}

#[cfg(any(test, feature = "test-util"))]
pub(super) fn write_bundles(
    discovered: &Target,
    requests: &[BundleRequest],
    executed: &ExecutedTests,
    reject_failed: bool,
) -> Result<Vec<CaptureReport>, ContextError> {
    write_with(discovered, requests, executed, reject_failed, &CommandEnvironment::real())
}

fn write_with(
    discovered: &Target,
    requests: &[BundleRequest],
    executed: &ExecutedTests,
    reject_failed: bool,
    system: &CommandEnvironment,
) -> Result<Vec<CaptureReport>, ContextError> {
    if reject_failed {
        reject_failures(executed)?;
    }

    let mut encoded = Vec::with_capacity(requests.len());
    for request in requests {
        encoded.push(encode_bundle(discovered, request, executed)?);
    }

    let mut reports = Vec::with_capacity(encoded.len());
    for bundle in encoded {
        system.create_dir_all(&bundle.out).map_err(|error| {
            ContextError::with_source(
                format!("failed to create capture output directory {}: {error}", bundle.out.display()),
                error,
            )
        })?;
        write_artifact(system, bundle.out.join(REGISTRY_FILE), &bundle.registry_bytes)?;
        write_artifact(system, bundle.out.join(TRACE_FILE), &bundle.trace_bytes)?;
        write_artifact(system, bundle.out.join(MANIFEST_FILE), &bundle.manifest_bytes)?;
        reports.push(bundle.report);
    }
    Ok(reports)
}
