//! Validated replay request and public report model.
//!
//! `capture_dir` owns the verified CTSC bundle, `binding` selects the candidate,
//! and the optional `out` is the atomically published candidate trace. When
//! `out` is omitted it defaults to
//! `<capture-dir>/candidate-<implementation>-<capture-dir name>.otlp.json`.
//! An existing output file is refused unless `force` is set.
//!
//! # Examples
//! ```
//! use specgate_cli::replay::{Paths, Request};
//! let request = Request::builder(Paths::new("capture", "candidate.binding.yaml"))
//!     .implementation("rust")
//!     .out("candidate.otlp.json")
//!     .build()?;
//! assert_eq!(request.implementation(), Some("rust"));
//! assert_eq!(request.capture_dir(), std::path::Path::new("capture"));
//! assert_eq!(request.out(), Some(std::path::Path::new("candidate.otlp.json")));
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
}
impl Paths {
    /// Construct replay paths from capture and candidate binding locations.
    pub fn new(capture_dir: impl AsRef<Path>, binding: impl AsRef<Path>) -> Self {
        Self {
            capture_dir: capture_dir.as_ref().to_path_buf(),
            binding: binding.as_ref().to_path_buf(),
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
    implementation: Option<specgate::ImplementationLabel>,
    // Optional paths cannot use the `path` projection, so the validated UTF-8 spelling is recorded.
    #[spec_event]
    out: Option<String>,
    #[spec_event]
    force: bool,
}
/// Staged construction for [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RequestBuilder {
    capture_dir: PathBuf,
    binding: PathBuf,
    implementation: Option<specgate::ImplementationLabel>,
    out: Option<PathBuf>,
    force: bool,
}
impl Request {
    /// Begin a request with capture and candidate binding paths.
    pub fn builder(paths: Paths) -> RequestBuilder {
        RequestBuilder {
            capture_dir: paths.capture_dir,
            binding: paths.binding,
            implementation: None,
            out: None,
            force: false,
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
    /// Candidate implementation label; `None` auto-selects the sole implementation.
    #[must_use]
    pub fn implementation(&self) -> Option<&str> {
        self.implementation.as_ref().map(specgate::ImplementationLabel::as_str)
    }
    /// Explicit candidate trace output path; `None` selects the default beside the capture bundle.
    #[must_use]
    pub fn out(&self) -> Option<&Path> {
        self.out.as_deref().map(Path::new)
    }
    /// Whether an existing output file may be overwritten.
    #[must_use]
    pub fn force(&self) -> bool {
        self.force
    }
}
impl RequestBuilder {
    /// Select a named candidate implementation.
    pub fn implementation(mut self, value: impl AsRef<str>) -> Self {
        self.implementation = Some(specgate::ImplementationLabel::from(value.as_ref()));
        self
    }
    /// Set the candidate trace output path.
    pub fn out(mut self, value: impl AsRef<Path>) -> Self {
        self.out = Some(value.as_ref().to_path_buf());
        self
    }
    /// Allow an existing output file to be overwritten.
    pub fn force(mut self, value: bool) -> Self {
        self.force = value;
        self
    }
    /// Validate and construct the request.
    ///
    /// # Errors
    /// Returns [`Error`] when a required path is empty or a path is not Unicode.
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
        let out = match self.out {
            None => None,
            Some(out) => {
                let Some(text) = out.to_str().filter(|text| !text.is_empty()) else {
                    return Err(Error::new_message(
                        ErrorKind::Request,
                        "replay output path must be non-empty valid UTF-8",
                    ));
                };
                if out.file_name().is_none() {
                    return Err(Error::new_message(ErrorKind::Request, "replay output path must end in a file name"));
                }
                Some(text.to_string())
            }
        };
        Ok(Request {
            capture_dir: self.capture_dir,
            binding: self.binding,
            implementation: self.implementation,
            out,
            force: self.force,
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
