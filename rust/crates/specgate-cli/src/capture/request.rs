//! Validated public capture request and report models.
//!
//! # Examples
//! ```
//! use specgate_cli::{CapturePaths, CaptureRequest};
//! let request = CaptureRequest::builder(CapturePaths {
//!     binding: "binding.yaml".into(),
//! }).implementation("rust").component("example.math").out("capture").build()?;
//! assert_eq!(request.implementation(), "rust");
//! assert_eq!(request.component(), "example.math");
//! assert_eq!(request.out(), Some(std::path::Path::new("capture")));
//! assert_eq!(request.binding(), std::path::Path::new("binding.yaml"));
//! # Ok::<(), specgate_cli::CaptureError>(())
//! ```
use super::{CaptureError, CaptureErrorKind, ComponentId, ImplementationLabel, Path, PathBuf, SpecEvent};

/// Summary of a capture run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "CaptureReport")]
pub struct CaptureReport {
    /// Captured component identifier.
    #[spec_event]
    pub(super) component_id: ComponentId,
    /// Number of captured test scenarios.
    #[spec_event]
    pub(super) scenarios: u32,
    /// Number of operation spans across captured scenarios.
    #[spec_event]
    pub(super) operations: u32,
    /// Written CTSC registry path.
    #[spec_event(path)]
    pub registry_path: PathBuf,
    /// Written OTLP reference trace path.
    #[spec_event(path)]
    pub trace_path: PathBuf,
    /// Written capture manifest path.
    #[spec_event(path)]
    pub manifest_path: PathBuf,
}

impl CaptureReport {
    /// Return the captured component identity as its CTSC string projection.
    #[must_use]
    pub fn component_id(&self) -> &str {
        self.component_id.as_str()
    }

    /// Number of captured test scenarios.
    #[must_use]
    pub fn scenarios(&self) -> u32 {
        self.scenarios
    }

    /// Number of operation spans across captured scenarios.
    #[must_use]
    pub fn operations(&self) -> u32 {
        self.operations
    }
}

/// Inputs for one public capture operation.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "CaptureRequest")]
pub struct CaptureRequest {
    /// Target binding file.
    #[spec_event(path)]
    binding: PathBuf,
    /// Implementation label, or empty to auto-select the component's sole implementation.
    #[spec_event]
    implementation: ImplementationLabel,
    /// Component identifier.
    #[spec_event]
    component: ComponentId,
    /// Explicit output directory, or `None` for the timestamped default.
    #[spec_event]
    out: Option<String>,
    /// Whether a non-empty output directory may be overwritten.
    #[spec_event]
    force: bool,
}

impl CaptureRequest {
    /// Begin constructing a request for the required binding path.
    ///
    /// The component is required before [`CaptureRequestBuilder::build`]; the
    /// implementation label defaults to empty, which auto-selects a sole
    /// implementation. Without [`CaptureRequestBuilder::out`] the bundle goes to
    /// `./.specgate/captures/<component>/<implementation>/<UTC run id>/`.
    pub fn builder(paths: CapturePaths) -> CaptureRequestBuilder {
        CaptureRequestBuilder {
            binding: paths.binding,
            implementation: ImplementationLabel::default(),
            component: ComponentId::default(),
            out: None,
            force: false,
        }
    }

    /// Target binding file.
    #[must_use]
    pub fn binding(&self) -> &Path {
        &self.binding
    }

    /// Implementation label, or empty to auto-select the component's sole implementation.
    #[must_use]
    pub fn implementation(&self) -> &str {
        self.implementation.as_str()
    }

    /// Component identifier.
    #[must_use]
    pub fn component(&self) -> &str {
        self.component.as_str()
    }

    /// Explicit output directory, or `None` for the timestamped default.
    #[must_use]
    pub fn out(&self) -> Option<&Path> {
        self.out.as_deref().map(Path::new)
    }

    /// Whether a non-empty output directory may be overwritten.
    #[must_use]
    pub fn force(&self) -> bool {
        self.force
    }

    pub(super) fn binding_str(&self) -> &str {
        self.binding
            .to_str()
            .expect("CaptureRequest binding is validated by its constructor")
    }
}

/// Required filesystem dependencies for a capture request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "capture path inputs are an intentionally direct public construction DTO"
)]
pub struct CapturePaths {
    /// Target binding file.
    pub binding: PathBuf,
}

/// Staged construction for a [`CaptureRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct CaptureRequestBuilder {
    binding: PathBuf,
    implementation: ImplementationLabel,
    component: ComponentId,
    out: Option<PathBuf>,
    force: bool,
}

impl CaptureRequestBuilder {
    /// Select a binding implementation by label. The default is an empty identity.
    pub fn implementation(mut self, label: impl AsRef<str>) -> Self {
        self.implementation = ImplementationLabel::from(label.as_ref());
        self
    }

    /// Select the component to capture. Required.
    pub fn component(mut self, component: impl AsRef<str>) -> Self {
        self.component = ComponentId::from(component.as_ref());
        self
    }

    /// Write the bundle to this directory. Its final path component becomes the
    /// capture target name recorded in the artifacts.
    pub fn out(mut self, out: impl AsRef<Path>) -> Self {
        self.out = Some(out.as_ref().to_path_buf());
        self
    }

    /// Allow publishing into a non-empty output directory.
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Validate the filesystem inputs and finish the request.
    ///
    /// # Errors
    ///
    /// Returns a request error when a path is not Unicode or is empty, when
    /// the output directory has no final name component, or when no component
    /// was selected.
    pub fn build(self) -> Result<CaptureRequest, CaptureError> {
        validate_utf8(&self.binding, "binding")?;
        if self.binding.as_os_str().is_empty() {
            return Err(CaptureError::message(
                CaptureErrorKind::Request,
                "capture requires a non-empty binding path",
            ));
        }
        let out = match self.out {
            Some(out) => {
                validate_utf8(&out, "output")?;
                if out.as_os_str().is_empty() {
                    return Err(CaptureError::message(
                        CaptureErrorKind::Request,
                        "capture requires a non-empty output directory",
                    ));
                }
                if out.file_name().is_none() {
                    return Err(CaptureError::message(
                        CaptureErrorKind::Request,
                        "capture output directory must end in a named directory",
                    ));
                }
                Some(
                    out.to_str()
                        .map(str::to_owned)
                        .ok_or_else(|| CaptureError::message(CaptureErrorKind::Request, "capture output path must be Unicode"))?,
                )
            }
            None => None,
        };
        if self.component.as_str().is_empty() {
            return Err(CaptureError::message(
                CaptureErrorKind::Request,
                "capture requires a component identifier",
            ));
        }
        Ok(CaptureRequest {
            binding: self.binding,
            implementation: self.implementation,
            component: self.component,
            out,
            force: self.force,
        })
    }
}

fn validate_utf8(path: impl AsRef<Path>, role: impl AsRef<str>) -> Result<(), CaptureError> {
    path.as_ref().to_str().map(|_| ()).ok_or_else(|| {
        CaptureError::message(
            CaptureErrorKind::Request,
            format!("capture {} path must be valid UTF-8", role.as_ref()),
        )
    })
}
