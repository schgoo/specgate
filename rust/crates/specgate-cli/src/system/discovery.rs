//! Binding and implementation discovery boundary.
#[cfg(any(test, feature = "test-util"))]
use std::collections::VecDeque;
use std::path::Path;
#[cfg(any(test, feature = "test-util"))]
use std::sync::{Arc, Mutex};

use specgate_discovery::identity::ComponentId;
use specgate_discovery::output::{Batch, Target};

/// Binding and compiled-metadata discovery with real and deterministic fake backends.
///
/// Use [`crate::Discovery::real`] for ordinary workflows. With the
/// `test-util` feature, [`crate::Discovery::fake`] accepts queued target or
/// batch results without reading host paths or environment state.
///
/// # Examples
/// ```
/// # #[cfg(feature = "test-util")] {
/// use specgate_cli::Discovery;
/// let discovery = Discovery::fake();
/// discovery.push_target(Err("injected discovery failure".into()));
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
    targets: VecDeque<Result<Target, Failure>>,
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

    pub(crate) fn target(&self, binding: impl AsRef<Path>, target: Option<&str>, component: &ComponentId) -> Result<Target, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_target(binding.as_ref(), target)?;
                Ok(specgate_discovery::discover_resolved(resolved, component)?)
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .targets
                .pop_front()
                .expect("fake target result queue must be configured before discovery"),
        }
    }

    /// Discover a capture target after enforcing capture's Rust-only constraint.
    pub(crate) fn capture_target(
        &self,
        binding: impl AsRef<Path>,
        target: Option<&str>,
        component: &ComponentId,
    ) -> Result<Target, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_target(binding.as_ref(), target)?;
                if resolved.language != specgate_discovery::binding::Language::Rust {
                    return Err(Failure::domain(format!(
                        "capture currently supports only Rust targets; binding language is '{}'",
                        resolved.language
                    )));
                }
                Ok(specgate_discovery::discover_resolved(resolved, component)?)
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .targets
                .pop_front()
                .expect("fake target result queue must be configured before discovery"),
        }
    }

    pub(crate) fn batch(
        &self,
        binding: impl AsRef<Path>,
        target: Option<&str>,
        components: impl AsRef<[ComponentId]>,
    ) -> Result<Batch, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_target(binding.as_ref(), target)?;
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
    pub fn push_target(&self, result: Result<Target, String>) {
        let Kind::Fake(state) = &self.inner else {
            panic!("push_target requires fake discovery")
        };
        state
            .lock()
            .expect("fake discovery lock poisoned")
            .targets
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
