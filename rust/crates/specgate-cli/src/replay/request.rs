//! Validated replay request and public report model.
//!
//! `capture_dir` owns the verified CTSC bundle, `binding` selects the candidate,
//! and `out` is the atomically published candidate trace.
//!
//! # Examples
//! ```
//! use specgate_cli::replay::{Paths, Request};
//! let request = Request::builder(Paths::new(
//!     "capture", "candidate.binding.yaml", "candidate.otlp.json",
//! )).target("rust").build()?;
//! assert_eq!(request.target(), Some("rust"));
//! assert_eq!(request.capture_dir(), std::path::Path::new("capture"));
//! # Ok::<(), specgate_cli::replay::Error>(())
//! ```
use super::{Error, ErrorKind, Path, PathBuf, SpecEvent};
use specgate::ComponentId;

/// Required filesystem locations for one replay request.
///
/// Values remain operating-system paths and are validated when the request is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    capture_dir: PathBuf,
    binding: PathBuf,
    out: PathBuf,
}
impl Paths {
    /// Construct replay paths from capture, candidate binding, and output locations.
    pub fn new(capture_dir: impl AsRef<Path>, binding: impl AsRef<Path>, out: impl AsRef<Path>) -> Self {
        Self {
            capture_dir: capture_dir.as_ref().to_path_buf(),
            binding: binding.as_ref().to_path_buf(),
            out: out.as_ref().to_path_buf(),
        }
    }
}
/// Owned inputs for one candidate replay operation.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "ReplayRequest")]
pub struct Request {
    #[spec_event(path)]
    capture_dir: PathBuf,
    #[spec_event(path)]
    binding: PathBuf,
    #[spec_event]
    target: Option<specgate::TargetName>,
    #[spec_event(path)]
    out: PathBuf,
}
/// Staged construction for [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RequestBuilder {
    capture_dir: PathBuf,
    binding: PathBuf,
    target: Option<specgate::TargetName>,
    out: PathBuf,
}
impl Request {
    /// Begin a request with capture, candidate binding, and output paths.
    pub fn builder(paths: Paths) -> RequestBuilder {
        RequestBuilder {
            capture_dir: paths.capture_dir,
            binding: paths.binding,
            target: None,
            out: paths.out,
        }
    }
    /// Capture bundle directory.
    #[must_use]
    pub fn capture_dir(&self) -> &Path {
        &self.capture_dir
    }
    /// Candidate binding path.
    #[must_use]
    pub fn binding(&self) -> &Path {
        &self.binding
    }
    /// Candidate target name.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_ref().map(specgate::TargetName::as_str)
    }
    /// Candidate trace output path.
    #[must_use]
    pub fn out(&self) -> &Path {
        &self.out
    }
}
impl RequestBuilder {
    /// Select a named candidate target.
    pub fn target(mut self, value: impl AsRef<str>) -> Self {
        self.target = Some(specgate::TargetName::from(value.as_ref()));
        self
    }
    /// Validate and construct the request.
    ///
    /// # Errors
    /// Returns [`Error`] when a required path is empty or the binding is not Unicode.
    pub fn build(self) -> Result<Request, Error> {
        if self.capture_dir.as_os_str().is_empty() {
            return Err(Error::new_message(
                ErrorKind::Request,
                "replay requires a non-empty capture directory",
            ));
        }
        if self.binding.as_os_str().is_empty() {
            return Err(Error::new_message(ErrorKind::Request, "replay requires a non-empty binding path"));
        }
        if self.binding.to_str().is_none() {
            return Err(Error::new_message(ErrorKind::Request, "replay binding path must be valid UTF-8"));
        }
        if self.out.as_os_str().is_empty() {
            return Err(Error::new_message(ErrorKind::Request, "replay requires a non-empty output path"));
        }
        Ok(Request {
            capture_dir: self.capture_dir,
            binding: self.binding,
            target: self.target,
            out: self.out,
        })
    }
}
/// Summary of a replay run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "ReplayReport")]
pub struct Report {
    /// Replayed component identifier.
    #[spec_event]
    pub(super) component_id: ComponentId,
    /// Number of replayed scenarios.
    #[spec_event]
    pub scenarios: u32,
    /// Number of invoked operations.
    #[spec_event]
    pub operations: u32,
    /// Number of distinct links.
    #[spec_event]
    pub plans: u32,
    /// Written candidate trace path.
    #[spec_event(path)]
    pub output_path: PathBuf,
}

impl Report {
    /// Return the replayed component identity as its CTSC string projection.
    #[must_use]
    pub fn component_id(&self) -> &str {
        self.component_id.as_str()
    }
}

/// Stable source spelling consumed by operation metadata.
pub(super) type ReplayRequest = Request;
/// Stable source spelling consumed by operation metadata.
pub(super) type ReplayReport = Report;
