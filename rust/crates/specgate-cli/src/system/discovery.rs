//! Binding and implementation discovery boundary.
#[cfg(any(test, feature = "test-util"))]
use std::collections::VecDeque;
use std::path::Path;
#[cfg(any(test, feature = "test-util"))]
use std::sync::{Arc, Mutex};

use specgate_discovery::identity::ComponentId;
use specgate_discovery::output::{Batch, Discovered};

/// Reject implementations that capture cannot run yet.
///
/// Capture builds and runs the Rust `sources` directory with Cargo. Other
/// languages, and implementations whose tests are declared as a free-form
/// `tests.command`, are rejected before any discovery work begins.
fn require_capturable(resolved: &specgate_discovery::binding::ResolvedImplementation) -> Result<(), Failure> {
    let language = resolved.language();
    if language != specgate_discovery::binding::Language::Rust {
        return Err(Failure::domain(format!(
            "capture currently supports only Rust implementations; implementation '{}' of component '{}' is '{language}'",
            resolved.label, resolved.component
        )));
    }
    if matches!(resolved.implementation.tests, specgate_discovery::binding::Tests::Command(_)) {
        return Err(Failure::domain(format!(
            "capture of implementation '{}' of component '{}' declares tests.command, which is not yet supported (#46)",
            resolved.label, resolved.component
        )));
    }
    Ok(())
}

/// Binding and compiled-metadata discovery with real and deterministic fake backends.
///
/// Use [`crate::Discovery::real`] for ordinary workflows. With the
/// `test-util` feature, [`crate::Discovery::fake`] accepts queued discovery or
/// batch results without reading host paths or environment state.
///
/// # Examples
/// ```
/// # #[cfg(feature = "test-util")] {
/// use specgate_cli::Discovery;
/// let discovery = Discovery::fake();
/// discovery.push_discovered(Err("injected discovery failure".into()));
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Discovery {
    inner: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Arc<Mutex<Fake>>),
}

#[cfg(any(test, feature = "test-util"))]
#[derive(Debug, Default)]
struct Fake {
    discovered: VecDeque<Result<Discovered, Failure>>,
    batches: VecDeque<Result<Batch, Failure>>,
}

#[ohno::error]
#[display("{diagnostic}")]
pub(crate) struct Failure {
    diagnostic: String,
}
impl Failure {
    fn domain(diagnostic: impl Into<String>) -> Self {
        Self::new(diagnostic.into())
    }
}
#[cfg(any(test, feature = "test-util"))]
fn fake_result<T>(result: Result<T, String>) -> Result<T, Failure> {
    Ok(result?)
}
impl From<specgate_discovery::Error> for Failure {
    fn from(error: specgate_discovery::Error) -> Self {
        Self::caused_by(error.to_string(), error)
    }
}
#[cfg(any(test, feature = "test-util"))]
impl From<String> for Failure {
    fn from(diagnostic: String) -> Self {
        Self::domain(diagnostic)
    }
}

impl Discovery {
    /// Select binding resolution and compiled implementation discovery.
    #[must_use]
    pub const fn real() -> Self {
        Self { inner: Kind::Real }
    }

    /// Discover the metadata of one component's implementation.
    pub(crate) fn discover(&self, binding: impl AsRef<Path>, component: &ComponentId, label: Option<&str>) -> Result<Discovered, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_implementation(binding.as_ref(), component, label)?;
                Ok(specgate_discovery::discover_resolved(resolved)?)
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .discovered
                .pop_front()
                .expect("fake discovery result queue must be configured before discovery"),
        }
    }

    /// Discover a capture implementation after enforcing capture's supported-implementation constraints.
    pub(crate) fn capture_discover(
        &self,
        binding: impl AsRef<Path>,
        component: &ComponentId,
        label: Option<&str>,
    ) -> Result<Discovered, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_implementation(binding.as_ref(), component, label)?;
                require_capturable(&resolved)?;
                Ok(specgate_discovery::discover_resolved(resolved)?)
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .discovered
                .pop_front()
                .expect("fake discovery result queue must be configured before discovery"),
        }
    }

    pub(crate) fn batch(
        &self,
        binding: impl AsRef<Path>,
        anchor: &ComponentId,
        label: Option<&str>,
        components: impl AsRef<[ComponentId]>,
    ) -> Result<Batch, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_implementation(binding.as_ref(), anchor, label)?;
                Ok(specgate_discovery::discover_many(resolved, components.as_ref())?)
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .batches
                .pop_front()
                .expect("fake batch result queue must be configured before discovery"),
        }
    }

    /// Construct an empty deterministic fake discovery service.
    #[cfg(any(test, feature = "test-util"))]
    #[must_use]
    pub fn fake() -> Self {
        Self {
            inner: Kind::Fake(Arc::new(Mutex::new(Fake::default()))),
        }
    }

    /// Queue one single-component discovery result for the fake backend.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn push_discovered(&self, result: Result<Discovered, String>) {
        let Kind::Fake(state) = &self.inner else {
            panic!("push_discovered requires fake discovery")
        };
        state
            .lock()
            .expect("fake discovery lock poisoned")
            .discovered
            .push_back(fake_result(result));
    }

    /// Queue one multi-component discovery result for the fake backend.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn push_batch(&self, result: Result<Batch, String>) {
        let Kind::Fake(state) = &self.inner else {
            panic!("push_batch requires fake discovery")
        };
        state
            .lock()
            .expect("fake discovery lock poisoned")
            .batches
            .push_back(fake_result(result));
    }
}
