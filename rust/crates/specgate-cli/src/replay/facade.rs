//! Public replay workflow orchestration.
use super::error::ReplayError;
use super::request::{ReplayReport, ReplayRequest};
use super::{
    CommandEnvironment, Discovery, Error, ErrorKind, Execution, Path, Report, Request, build_plan, decode_bundle, discover_candidate,
    execute_with, manifest_file, reference_file, registry_file, spec_operation, write,
};
/// Replay every top-level operation in a verified capture bundle.
///
/// # Errors
/// Returns an opaque error enriched with loading, linking, execution, or
/// publication context.
///
/// # Examples
/// ```no_run
/// use specgate_cli::replay::{Paths, Request, replay};
/// let request = Request::builder(Paths::new("capture", "candidate.binding.yaml", "candidate.otlp.json")).build()?;
/// let report = replay(request)?;
/// assert_eq!(report.output_path, std::path::Path::new("candidate.otlp.json"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[spec_operation("replay")]
pub fn replay(request: ReplayRequest) -> Result<ReplayReport, ReplayError> {
    replay_with(&request, &Execution::real(), &Discovery::real(), &CommandEnvironment::real())
}

/// Replay through caller-supplied filesystem, execution, and discovery services.
///
/// The services control bundle I/O, generated-runner processes, and candidate
/// metadata discovery. Production callers normally use [`replay`]; tests with
/// `test-util` can inject deterministic fake backends.
///
/// # Examples
/// ```no_run
/// use specgate_cli::{Discovery, Execution, CommandEnvironment};
/// use specgate_cli::replay::{replay_with, Paths, Request};
/// let request = Request::builder(Paths::new("capture", "candidate.yaml", "candidate.json")).build()?;
/// let report = replay_with(&request, &Execution::real(), &Discovery::real(), &CommandEnvironment::real())?;
/// assert!(report.output_path.ends_with("candidate.json"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
/// Returns a categorized replay failure from loading, linking, execution, or publication.
pub fn replay_with(request: &Request, execution: &Execution, discovery: &Discovery, system: &CommandEnvironment) -> Result<Report, Error> {
    let manifest = read_bundle(request.capture_dir(), manifest_file(), system)?;
    let registry = read_bundle(request.capture_dir(), registry_file(), system)?;
    let reference = read_bundle(request.capture_dir(), reference_file(), system)?;
    let bundle =
        decode_bundle(&manifest, &registry, &reference).map_err(|source| Error::wrap(ErrorKind::Capture, source.to_string(), source))?;
    let target = request.target().map(specgate_discovery::identity::TargetName::from);
    let candidate = discover_candidate(request.binding(), target.as_ref(), &bundle.component_id, discovery, system)?;
    let plan = build_plan(&bundle, &candidate)?;
    let captures = execute_with(&plan, execution, system)
        .map_err(|error| Error::wrap(ErrorKind::Execution, "candidate replay execution failed", error))?;
    let mut report = write(&plan, &captures, request.out(), system)
        .map_err(|error| Error::wrap(ErrorKind::Publication, "candidate replay publication failed", error))?;
    report.output_path = request.out().to_path_buf();
    Ok(report)
}

pub(super) fn read_bundle(
    capture_dir: impl AsRef<Path>,
    filename: impl AsRef<Path>,
    system: &CommandEnvironment,
) -> Result<Vec<u8>, Error> {
    let path = capture_dir.as_ref().join(filename);
    system.read(&path).map_err(|error| {
        Error::wrap(
            ErrorKind::Capture,
            format!("failed to read capture bundle file {}", path.display()),
            error,
        )
    })
}
