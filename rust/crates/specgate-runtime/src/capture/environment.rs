//! Lazy environment-driven native-capture activation.

use super::{EnvConfig, FileSystem, current_collector, is_active, start_with};
use crate::CaptureError;
use std::cell::{Cell, RefCell};
use std::ffi::{OsStr, OsString};

/// Parent-process environment key carrying native capture activation JSON.
/// Changing this name breaks CLI-to-runtime capture activation compatibility.
const ENV_KEY: &str = "SPECGATE_NATIVE_CAPTURE";

#[derive(Default)]
struct Environment {
    backend: EnvironmentBackend,
}

#[derive(Default)]
enum EnvironmentBackend {
    #[default]
    Real,
    #[cfg(feature = "test-util")]
    Fake(std::collections::BTreeMap<OsString, OsString>),
}

impl Environment {
    fn var_os(&self, name: impl AsRef<OsStr>) -> Option<OsString> {
        let name = name.as_ref();
        match &self.backend {
            EnvironmentBackend::Real => std::env::var_os(name),
            #[cfg(feature = "test-util")]
            EnvironmentBackend::Fake(values) => values.get(name).cloned(),
        }
    }

    const fn is_real(&self) -> bool {
        matches!(self.backend, EnvironmentBackend::Real)
    }
}

thread_local! {
    static CAPTURE_ENVIRONMENT: RefCell<Environment> = RefCell::new(Environment::default());
    static REAL_ENV_CHECKED: Cell<bool> = const { Cell::new(false) };
}

fn env_var(name: impl AsRef<OsStr>) -> Option<OsString> {
    CAPTURE_ENVIRONMENT.with(|environment| environment.borrow().var_os(name))
}

fn capture_env_var() -> Option<OsString> {
    CAPTURE_ENVIRONMENT.with(|environment| {
        let environment = environment.borrow();
        if environment.is_real() {
            if REAL_ENV_CHECKED.with(Cell::get) {
                return None;
            }
            REAL_ENV_CHECKED.with(|checked| checked.set(true));
        }
        environment.var_os(ENV_KEY)
    })
}

pub(super) fn requested() -> bool {
    is_active() || env_var(ENV_KEY).is_some_and(|value| !value.is_empty())
}

pub(super) fn activate_env() -> Result<(), CaptureError> {
    if current_collector().is_some() {
        return Ok(());
    }
    let Some(encoded) = capture_env_var() else {
        return Ok(());
    };
    if encoded.is_empty() {
        return Ok(());
    }
    let environment: EnvConfig = serde_json::from_str(&encoded.to_string_lossy())
        .map_err(|error| format!("invalid SPECGATE_NATIVE_CAPTURE configuration: {error}"))?;
    start_with(environment.capture, Some(environment.sidecar_path), FileSystem::real())
}

#[cfg(feature = "test-util")]
pub mod test_util {
    //! Thread-local deterministic environment controls for integration tests.
    //! Values affect lazy native-capture activation on the calling thread only.

    pub use super::super::io::persistence_stage::PersistenceStage;
    use super::super::{Config, FakeFs, FileSystem, start_with};
    use super::{CAPTURE_ENVIRONMENT, Environment, EnvironmentBackend, REAL_ENV_CHECKED};
    pub use crate::generated::Output;
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Controller for one injected native-capture filesystem.
    #[derive(Debug, Clone)]
    pub struct PersistenceProbe {
        state: Arc<Mutex<FakeFs>>,
    }

    impl PersistenceProbe {
        /// Return the ordered persistence stages attempted so far.
        ///
        /// # Panics
        /// Panics if a test poisoned the fake filesystem mutex.
        #[must_use]
        pub fn calls(&self) -> Vec<PersistenceStage> {
            self.state.lock().expect("fake filesystem mutex poisoned").calls.clone()
        }

        /// Return every successfully replaced sidecar snapshot.
        ///
        /// # Panics
        /// Panics if a test poisoned the fake filesystem mutex.
        #[must_use]
        pub fn snapshots(&self) -> Vec<Vec<u8>> {
            self.state.lock().expect("fake filesystem mutex poisoned").snapshots.clone()
        }
    }

    /// Start capture with an injected filesystem and optional failing stage.
    ///
    /// # Errors
    /// Returns the same typed configuration errors as ordinary capture
    /// activation, and rejects an empty sidecar path.
    pub fn start_with_fs(
        config: Config,
        sidecar: impl Into<PathBuf>,
        fail_stage: Option<PersistenceStage>,
    ) -> Result<PersistenceProbe, crate::CaptureError> {
        let (file_system, state) = FileSystem::fake(FakeFs {
            fail_stage,
            ..FakeFs::default()
        });
        start_with(config, Some(sidecar.into()), file_system)?;
        Ok(PersistenceProbe { state })
    }

    /// Capture generated parent-protocol output without writing process stderr.
    #[must_use]
    pub fn capture_output() -> Output {
        crate::generated::capture_output()
    }

    /// Restore real parent-protocol output for this thread.
    pub fn reset_output() {
        crate::generated::reset_output();
    }

    /// Replace the environment visible to native-capture activation on this thread.
    pub fn set_environment(values: impl IntoIterator<Item = (OsString, OsString)>) {
        CAPTURE_ENVIRONMENT.with(|environment| {
            *environment.borrow_mut() = Environment {
                backend: EnvironmentBackend::Fake(values.into_iter().collect::<BTreeMap<_, _>>()),
            };
        });
        REAL_ENV_CHECKED.with(|checked| checked.set(false));
    }

    /// Restore access to the process environment on this thread.
    pub fn reset_environment() {
        CAPTURE_ENVIRONMENT.with(|environment| *environment.borrow_mut() = Environment::default());
        REAL_ENV_CHECKED.with(|checked| checked.set(false));
    }

    /// Convenience key conversion for callers assembling deterministic values.
    #[must_use]
    pub fn key(value: impl AsRef<OsStr>) -> OsString {
        value.as_ref().to_os_string()
    }
}
