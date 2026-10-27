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
/// Implementations whose tests are declared as a free-form `tests.command` are
/// rejected before any discovery work begins; language support is decided by
/// the capture runner that owns the language.
fn require_capturable(resolved: &specgate_discovery::binding::ResolvedImplementation) -> Result<(), Failure> {
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

    /// Resolve the implementation a capture will run, without building it.
    ///
    /// Rejects what capture cannot run yet. The fake backend reports the
    /// implementation of its next queued result, or that result's failure.
    pub(crate) fn capture_resolve(
        &self,
        binding: impl AsRef<Path>,
        component: &ComponentId,
        label: Option<&str>,
    ) -> Result<specgate_discovery::binding::ResolvedImplementation, Failure> {
        let resolved = self.resolve(binding, component, label)?;
        require_capturable(&resolved)?;
        Ok(resolved)
    }

    /// Resolve sources before acquiring package ownership or building metadata.
    pub(crate) fn resolve(
        &self,
        binding: impl AsRef<Path>,
        component: &ComponentId,
        label: Option<&str>,
    ) -> Result<specgate_discovery::binding::ResolvedImplementation, Failure> {
        match &self.inner {
            Kind::Real => {
                let resolved = specgate_discovery::binding::resolve_implementation(binding.as_ref(), component, label)?;
                Ok(resolved)
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => {
                let mut fake = state.lock().expect("fake discovery lock poisoned");
                if let Some(front) = fake.discovered.front() {
                    let Ok(discovered) = front else {
                        return match fake.discovered.pop_front() {
                            Some(Err(error)) => Err(error),
                            _ => unreachable!("front was just seen as an error"),
                        };
                    };
                    return Ok(discovered.implementation.clone());
                }
                let front = fake
                    .batches
                    .front()
                    .expect("fake discovery result queue must be configured before discovery");
                let Ok(batch) = front else {
                    return match fake.batches.pop_front() {
                        Some(Err(error)) => Err(error),
                        _ => unreachable!("front was just seen as an error"),
                    };
                };
                Ok(batch.implementation.clone())
            }
        }
    }

    /// Build and discover one resolved implementation.
    pub(crate) fn discover_resolved(&self, resolved: specgate_discovery::binding::ResolvedImplementation) -> Result<Discovered, Failure> {
        match &self.inner {
            Kind::Real => Ok(specgate_discovery::discover_resolved(resolved)?),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .discovered
                .pop_front()
                .expect("fake discovery result queue must be configured before discovery"),
        }
    }

    /// Reflect every component of a C# implementation from an assembly a
    /// capture build already produced under `artifacts`, building nothing.
    pub(crate) fn discover_built(
        &self,
        resolved: specgate_discovery::binding::ResolvedImplementation,
        artifacts: &Path,
    ) -> Result<Batch, Failure> {
        match &self.inner {
            Kind::Real => Ok(specgate_discovery::discover_built(resolved, artifacts)?),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => state
                .lock()
                .expect("fake discovery lock poisoned")
                .batches
                .pop_front()
                .expect("fake batch result queue must be configured before discovery"),
        }
    }

    /// Discover a batch after the caller has acquired package ownership.
    pub(crate) fn batch_resolved(
        &self,
        resolved: specgate_discovery::binding::ResolvedImplementation,
        components: impl AsRef<[ComponentId]>,
    ) -> Result<Batch, Failure> {
        match &self.inner {
            Kind::Real => Ok(specgate_discovery::discover_many(resolved, components.as_ref())?),
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
