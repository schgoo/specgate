#![cfg_attr(all(feature = "test-util", not(test)), expect(dead_code, reason = "feature-gated fixture support"))]

//! Capture workflow orchestration and publication.
use super::{
    CaptureError, CaptureErrorKind, CaptureReport, CaptureRequest, CommandEnvironment, ComponentId, Discovered, Discovery, ExecutedTests,
    Execution, FailureContext, MANIFEST_FILE, Path, PathBuf, REGISTRY_FILE, TRACE_FILE, csharp, encode_bundle, public_error,
    reject_failures, run_id, rust, scratch_dir, spec_operation, validate_setups, write_artifact,
};

// Default capture bundles live beside the project, outside version control via `.gitignore`.
const DEFAULT_CAPTURE_ROOT: &str = ".specgate/captures";

// Ordinary capture publishes evidence from failing tests; golden generation opts into rejecting them.
const REJECT_FAILURES: bool = false;

/// Capture passing tests for one Rust or C# implementation of a component as a native CTSC bundle.
///
/// An empty implementation label auto-selects the component's sole
/// implementation and errors when several are declared. The component is
/// required.
///
/// Without an explicit output directory the bundle is written to
/// `.specgate/captures/<component>/<implementation>/<UTC run id>/` under the
/// current directory. The capture target name recorded in the manifest and
/// trace is the output directory's final path component. A non-empty output
/// directory is refused unless the request sets `force`.
///
/// # Errors
///
/// Returns an opaque [`crate::CaptureError`] with the failed operation and any
/// upstream cause preserved in its enrichment chain.
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
///     binding: "specgate.yaml".into(),
/// })
///     .implementation("rust")
///     .component("example.math")
///     .out("capture")
///     .build()?;
/// let report = capture(request)?;
/// println!("{}", report.manifest_path.display());
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
/// let request = CaptureRequest::builder(CapturePaths { binding: "specgate.yaml".into() }).component("example.math").out("capture").build()?;
/// let report = capture_with(&request, &CommandEnvironment::real(), &Execution::real(), &Discovery::real())?;
/// assert!(report.manifest_path.ends_with("manifest.json"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
/// Returns an opaque capture failure from request execution or publication.
///
/// # Panics
/// Panics if the internal bundle writer violates its one-report-per-request invariant.
pub fn capture_with(
    request: &CaptureRequest,
    system: &CommandEnvironment,
    execution: &Execution,
    discovery: &Discovery,
) -> Result<CaptureReport, CaptureError> {
    let component = ComponentId::from(request.component());
    let label = Some(request.implementation()).filter(|label| !label.is_empty());
    let resolved = discovery
        .capture_resolve(
            request.binding_str(),
            &specgate_discovery::identity::ComponentId::from(component.as_str()),
            label,
        )
        .map_err(|error| FailureContext::with_source(error.to_string(), error))
        .map_err(public_error(CaptureErrorKind::Discovery))?;
    let _package = system
        .package_guard(&resolved.implementation.sources)
        .map_err(|error| FailureContext::with_source(error.to_string(), error))
        .map_err(public_error(CaptureErrorKind::Execution))?;
    let out = request
        .out()
        .map_or_else(|| default_out(&component, &resolved.label, system), Path::to_path_buf);
    if !request.force() && system.is_occupied(&out) {
        return Err(public_error(CaptureErrorKind::Request)(FailureContext::domain(format!(
            "capture output directory {} is not empty; pass --force to overwrite it",
            out.display()
        ))));
    }
    let requests = [BundleRequest {
        component,
        out,
        #[cfg(any(test, feature = "test-util"))]
        excluded_operations: std::collections::BTreeSet::new(),
    }];
    let (discovered, executed) = capture_tests(resolved, &requests, system, execution, discovery)?;
    let reports =
        write_with(&discovered, &requests, &executed, REJECT_FAILURES, system).map_err(public_error(CaptureErrorKind::Encoding))?;
    Ok(reports
        .into_iter()
        .next()
        .expect("one capture bundle request must produce one publication report"))
}

/// Discover the implementation's metadata and run its tests, once for the batch.
///
/// A Rust implementation is discovered first and then built and run. A C#
/// implementation's metadata comes from the build that weaves and runs its
/// tests, so discovery cannot precede it and the sources are never compiled a
/// second time.
fn capture_tests(
    resolved: specgate_discovery::binding::ResolvedImplementation,
    requests: &[BundleRequest],
    system: &CommandEnvironment,
    execution: &Execution,
    discovery: &Discovery,
) -> Result<(Discovered, ExecutedTests), CaptureError> {
    match resolved.language() {
        specgate_discovery::binding::Language::Rust => {
            let discovered = discover_rust(resolved, system, discovery).map_err(public_error(CaptureErrorKind::Discovery))?;
            let executed = execute_with(&discovered, requests, system, execution).map_err(public_error(CaptureErrorKind::Execution))?;
            Ok((discovered, executed))
        }
        specgate_discovery::binding::Language::CSharp => {
            let sources = resolved.implementation.sources.clone();
            let scratch = scratch_dir(system, sources).map_err(public_error(CaptureErrorKind::Execution))?;
            csharp::capture(system, execution, discovery, &resolved, requests, scratch.path())
                .map_err(public_error(CaptureErrorKind::Execution))
        }
    }
}

/// Default bundle directory: `.specgate/captures/<component>/<implementation>/<UTC run id>`.
///
/// The run id comes from the injected system clock, so the path is
/// deterministic under a fake clock.
pub(super) fn default_out(
    component: &ComponentId,
    label: &specgate_discovery::identity::ImplementationLabel,
    system: &CommandEnvironment,
) -> PathBuf {
    Path::new(DEFAULT_CAPTURE_ROOT)
        .join(component.as_str())
        .join(label.as_str())
        .join(run_id(system.now()))
}

/// One requested component bundle within a batched capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleRequest {
    pub(crate) component: ComponentId,
    pub(crate) out: PathBuf,
    /// Golden-only operation exclusions. Product capture always leaves this
    /// empty; the golden matrix validates every declared exclusion separately.
    #[cfg(any(test, feature = "test-util"))]
    pub(crate) excluded_operations: std::collections::BTreeSet<String>,
}

/// Capture many components from one Rust implementation in a single pass.
///
/// Discovery, the libtest build, test enumeration, and test execution each
/// happen exactly once for the whole batch; only bundle encoding is per
/// component. [`capture`] is the one-component case of this same path, so
/// batched and single bundles are byte-identical.
///
/// Callers that already hold a [`Discovered`] should use
/// [`capture_discovered`] directly and pay discovery once for the whole run.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_many(
    binding: impl AsRef<Path>,
    component: &ComponentId,
    label: Option<&str>,
    requests: &[BundleRequest],
) -> Result<Vec<CaptureReport>, FailureContext> {
    assert!(!requests.is_empty(), "capture_many requires at least one bundle request");
    let discovered = discover(binding, component, label)?;
    let present = discovered.registry.present_components();
    for request in requests {
        if !present.iter().any(|candidate| candidate == request.component.as_str()) {
            return Err(FailureContext::domain(format!(
                "component '{}' not found; available components: {}",
                request.component,
                present.join(", ")
            )));
        }
    }
    capture_discovered(&discovered, requests)
}

#[cfg(any(test, feature = "test-util"))]
pub(crate) fn discover(binding: impl AsRef<Path>, component: &ComponentId, label: Option<&str>) -> Result<Discovered, FailureContext> {
    let discovery = Discovery::real();
    let resolved = discovery
        .capture_resolve(
            binding.as_ref(),
            &specgate_discovery::identity::ComponentId::from(component.as_str()),
            label,
        )
        .map_err(|error| FailureContext::with_source(error.to_string(), error))?;
    discover_rust(resolved, &CommandEnvironment::real(), &discovery)
}

/// Capture a C# implementation through the real build, weaver, and test runner.
///
/// Returns the selected component's discovery result and the executed tests, ready
/// for [`write_bundles`]. The sources are compiled once per tests project.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_csharp_tests(
    binding: impl AsRef<Path>,
    component: &ComponentId,
    label: Option<&str>,
    requests: &[BundleRequest],
) -> Result<(Discovered, ExecutedTests), FailureContext> {
    let discovery = Discovery::real();
    let resolved = discovery
        .capture_resolve(
            binding.as_ref(),
            &specgate_discovery::identity::ComponentId::from(component.as_str()),
            label,
        )
        .map_err(|error| FailureContext::with_source(error.to_string(), error))?;
    let system = CommandEnvironment::real();
    let scratch = scratch_dir(&system, &resolved.implementation.sources)?;
    csharp::capture(&system, &Execution::real(), &discovery, &resolved, requests, scratch.path())
}

/// Golden-only strict C# batch. Keep executed-test internals inside capture;
/// metadata and bundles both come from the one woven tests build.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_csharp_strict(
    binding: impl AsRef<Path>,
    component: &ComponentId,
    label: Option<&str>,
    requests: &[BundleRequest],
) -> Result<std::collections::BTreeMap<String, String>, FailureContext> {
    let (discovered, executed) = capture_csharp_tests(binding, component, label, requests)?;
    let reports = write_bundles(&discovered, requests, &executed, true)?;
    assert_eq!(reports.len(), requests.len(), "C# capture must return one report per request");
    executed
        .schemas
        .into_iter()
        .map(|(component, schema)| {
            let schema = schema.map_err(FailureContext::domain)?;
            Ok((component, schema))
        })
        .collect()
}

/// Discover a Rust capture implementation's metadata.
///
/// C# metadata is reflected from the capture build itself, so a C#
/// implementation never reaches this function in a capture.
fn discover_rust(
    resolved: specgate_discovery::binding::ResolvedImplementation,
    system: &CommandEnvironment,
    discovery: &Discovery,
) -> Result<Discovered, FailureContext> {
    if resolved.language() != specgate_discovery::binding::Language::Rust {
        return Err(FailureContext::domain(format!(
            "implementation '{}' is not a Rust implementation; C# metadata is discovered by the capture build",
            resolved.label
        )));
    }
    if !system.is_file(resolved.implementation.sources.join("Cargo.toml")) {
        return Err(FailureContext::domain(format!(
            "implementation '{}' is not a Rust package (no Cargo.toml at {})",
            resolved.label,
            resolved.implementation.sources.display()
        )));
    }
    discovery
        .discover_resolved(resolved)
        .map_err(|error| FailureContext::with_source(error.to_string(), error))
}

/// Capture many components, keeping the tests that pass.
///
/// This is `specgate capture`'s behavior: a failing test contributes no
/// scenario, and a component left with no scenario at all still errors.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_discovered(discovered: &Discovered, requests: &[BundleRequest]) -> Result<Vec<CaptureReport>, FailureContext> {
    discovered_with(discovered, requests, FailureMode::Allow)
}

/// Capture many components, failing on any failing enumerated fixture test.
///
/// Used by the CTSC golden harness, where a fixture test that fails is a
/// product or fixture defect rather than a scenario to skip: the goldens claim
/// to be the corpus's real behavior, so a red test must not be hidden by a
/// sibling test that happens to cover the same component.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn capture_strict(discovered: &Discovered, requests: &[BundleRequest]) -> Result<Vec<CaptureReport>, FailureContext> {
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
    discovered: &Discovered,
    requests: &[BundleRequest],
    failure_mode: FailureMode,
) -> Result<Vec<CaptureReport>, FailureContext> {
    let executed = execute_tests(discovered, requests)?;
    write_bundles(discovered, requests, &executed, matches!(failure_mode, FailureMode::Reject))
}

#[cfg(any(test, feature = "test-util"))]
pub(super) fn execute_tests(discovered: &Discovered, requests: &[BundleRequest]) -> Result<ExecutedTests, FailureContext> {
    execute_with(discovered, requests, &CommandEnvironment::real(), &Execution::real())
}

fn execute_with(
    discovered: &Discovered,
    requests: &[BundleRequest],
    system: &CommandEnvironment,
    execution: &Execution,
) -> Result<ExecutedTests, FailureContext> {
    let resolved = &discovered.implementation;
    validate_setups(&discovered.registry, requests)?;
    let sources = &resolved.implementation.sources;
    match resolved.language() {
        specgate_discovery::binding::Language::Rust => {
            let scratch = scratch_dir(system, sources)?;
            rust::execute(system, execution, sources, scratch.path())
        }
        specgate_discovery::binding::Language::CSharp => Err(FailureContext::domain(
            "C# tests run through the capture build, which also discovers the metadata; they cannot run from a prior discovery result",
        )),
    }
}

#[cfg(any(test, feature = "test-util"))]
pub(super) fn write_bundles(
    discovered: &Discovered,
    requests: &[BundleRequest],
    executed: &ExecutedTests,
    reject_failed: bool,
) -> Result<Vec<CaptureReport>, FailureContext> {
    write_with(discovered, requests, executed, reject_failed, &CommandEnvironment::real())
}

fn write_with(
    discovered: &Discovered,
    requests: &[BundleRequest],
    executed: &ExecutedTests,
    reject_failed: bool,
    system: &CommandEnvironment,
) -> Result<Vec<CaptureReport>, FailureContext> {
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
            FailureContext::with_source(
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
