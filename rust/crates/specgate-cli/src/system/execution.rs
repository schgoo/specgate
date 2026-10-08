//! Process execution boundary.
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
#[cfg(any(test, feature = "test-util"))]
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

/// Child-process execution with real and deterministic fake backends.
///
/// Ordinary workflows use [`crate::Execution::real`]. With `test-util`, a
/// fake can queue [`std::process::Output`] values and expose observed programs:
///
/// ```
/// # #[cfg(feature = "test-util")] {
/// use specgate_cli::Execution;
/// let execution = Execution::fake();
/// assert!(execution.observed_programs().is_empty());
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Execution {
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
    results: VecDeque<Result<Output, io::Error>>,
    requests: Vec<Request>,
}

/// Complete side-effect-free description of one child process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    program: OsString,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    environment: Vec<(OsString, Option<OsString>)>,
}
/// Staged construction for [`Request`].
#[derive(Clone, Debug)]
pub(crate) struct RequestBuilder {
    request: Request,
}
impl Request {
    /// Start describing a process with its required executable.
    pub(crate) fn builder(program: impl AsRef<OsStr>) -> RequestBuilder {
        RequestBuilder {
            request: Self {
                program: program.as_ref().to_os_string(),
                args: Vec::new(),
                current_dir: None,
                environment: Vec::new(),
            },
        }
    }
}
impl RequestBuilder {
    /// Append one command argument.
    pub(crate) fn arg(mut self, value: impl AsRef<OsStr>) -> Self {
        self.request.args.push(value.as_ref().to_os_string());
        self
    }
    /// Append command arguments in order.
    pub(crate) fn args(mut self, values: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        self.request
            .args
            .extend(values.into_iter().map(|value| value.as_ref().to_os_string()));
        self
    }
    /// Set the child working directory.
    pub(crate) fn current_dir(mut self, value: impl AsRef<Path>) -> Self {
        self.request.current_dir = Some(value.as_ref().to_path_buf());
        self
    }
    /// Set one child environment value.
    pub(crate) fn env(mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.request
            .environment
            .push((name.as_ref().to_os_string(), Some(value.as_ref().to_os_string())));
        self
    }
    /// Remove one inherited environment value.
    pub(crate) fn env_remove(mut self, name: impl AsRef<OsStr>) -> Self {
        self.request.environment.push((name.as_ref().to_os_string(), None));
        self
    }
    /// Finish the immutable request.
    pub(crate) fn build(self) -> Request {
        self.request
    }
}
impl Execution {
    /// Select native child-process execution.
    #[must_use]
    pub const fn real() -> Self {
        Self { inner: Kind::Real }
    }
    /// Execute one complete request and capture all output.
    pub(crate) fn run(&self, request: &Request) -> io::Result<Output> {
        match &self.inner {
            Kind::Real => {
                let mut command = Command::new(&request.program);
                command.args(&request.args);
                if let Some(dir) = &request.current_dir {
                    command.current_dir(dir);
                }
                for (name, value) in &request.environment {
                    if let Some(value) = value {
                        command.env(name, value);
                    } else {
                        command.env_remove(name);
                    }
                }
                command.output()
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => {
                let mut fake = fake.lock().expect("fake execution lock poisoned");
                fake.requests.push(request.clone());
                fake.results
                    .pop_front()
                    .expect("fake process result queue must be configured before execution")
            }
        }
    }
    /// Construct an empty deterministic fake.
    #[cfg(any(test, feature = "test-util"))]
    #[must_use]
    pub fn fake() -> Self {
        Self {
            inner: Kind::Fake(Arc::new(Mutex::new(Fake::default()))),
        }
    }
    /// Queue one fake process result.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn push(&self, result: io::Result<Output>) {
        let Kind::Fake(fake) = &self.inner else {
            panic!("push requires fake execution")
        };
        fake.lock().expect("fake execution lock poisoned").results.push_back(result);
    }
    /// Return requests observed by the fake.
    #[cfg(any(test, feature = "test-util"))]
    pub(crate) fn requests(&self) -> Vec<Request> {
        let Kind::Fake(fake) = &self.inner else {
            panic!("requests requires fake execution")
        };
        fake.lock().expect("fake execution lock poisoned").requests.clone()
    }

    /// Return the executable names observed by the fake backend.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    #[must_use]
    pub fn observed_programs(&self) -> Vec<OsString> {
        self.requests().into_iter().map(|request| request.program).collect()
    }
}
