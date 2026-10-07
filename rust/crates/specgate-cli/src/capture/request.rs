//! Validated public capture request and report models.
//!
//! # Examples
//! ```
//! use specgate_cli::{CapturePaths, CaptureRequest};
//! let request = CaptureRequest::builder(CapturePaths {
//!     binding: "binding.yaml".into(), out: "capture".into(),
//! }).target("rust").component("example.math").build()?;
//! assert_eq!(request.target(), "rust");
//! assert_eq!(request.component(), "example.math");
//! assert_eq!(request.binding(), std::path::Path::new("binding.yaml"));
//! # Ok::<(), specgate_cli::CaptureError>(())
//! ```
use super::{CaptureError, CaptureErrorKind, ComponentId, Path, PathBuf, SpecEvent, TargetName};

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
    /// Binding target name, or empty to select the binding default.
    #[spec_event]
    target: TargetName,
    /// Component identifier, or empty to select the sole discovered component.
    #[spec_event]
    component: ComponentId,
    /// Output directory for the capture bundle.
    #[spec_event(path)]
    out: PathBuf,
}

impl CaptureRequest {
    /// Begin constructing a request for the required binding and output paths.
    ///
    /// Target and component default to empty identities, retaining default
    /// target and sole-component selection.
    pub fn builder(paths: CapturePaths) -> CaptureRequestBuilder {
        CaptureRequestBuilder {
            binding: paths.binding,
            target: TargetName::default(),
            component: ComponentId::default(),
            out: paths.out,
        }
    }

    /// Target binding file.
    #[must_use]
    pub fn binding(&self) -> &Path {
        &self.binding
    }

    /// Binding target name, or empty to select the binding default.
    #[must_use]
    pub fn target(&self) -> &str {
        self.target.as_str()
    }

    /// Component identifier, or empty to select the sole discovered component.
    #[must_use]
    pub fn component(&self) -> &str {
        self.component.as_str()
    }

    /// Output directory for the capture bundle.
    #[must_use]
    pub fn out(&self) -> &Path {
        &self.out
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
    /// Output directory for the capture bundle.
    pub out: PathBuf,
}

/// Staged construction for a [`CaptureRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct CaptureRequestBuilder {
    binding: PathBuf,
    target: TargetName,
    component: ComponentId,
    out: PathBuf,
}

impl CaptureRequestBuilder {
    /// Select a named binding target. The default is an empty identity.
    pub fn target(mut self, target: impl AsRef<str>) -> Self {
        self.target = TargetName::from(target.as_ref());
        self
    }

    /// Select a component. The default is an empty identity.
    pub fn component(mut self, component: impl AsRef<str>) -> Self {
        self.component = ComponentId::from(component.as_ref());
        self
    }

    /// Validate the filesystem inputs and finish the request.
    ///
    /// # Errors
    ///
    /// Returns a request error when either path is not Unicode or is empty.
    pub fn build(self) -> Result<CaptureRequest, CaptureError> {
        validate_utf8(&self.binding, "binding")?;
        if self.binding.as_os_str().is_empty() {
            return Err(CaptureError::message(
                CaptureErrorKind::Request,
                "capture requires a non-empty binding path",
            ));
        }
        validate_utf8(&self.out, "output")?;
        if self.out.as_os_str().is_empty() {
            return Err(CaptureError::message(
                CaptureErrorKind::Request,
                "capture requires a non-empty output directory",
            ));
        }
        Ok(CaptureRequest {
            binding: self.binding,
            target: self.target,
            component: self.component,
            out: self.out,
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
