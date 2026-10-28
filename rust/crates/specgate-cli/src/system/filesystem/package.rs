//! Nonblocking, process-scoped ownership of a canonical source package.

use super::{CommandEnvironment, Kind};
use sha2::{Digest as _, Sha256};
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct PackageGuard {
    _lease: Arc<Lease>,
}

#[derive(Debug)]
enum Lease {
    Real {
        _file: File,
    },
    #[cfg(any(test, feature = "test-util"))]
    Fake {
        package: std::path::PathBuf,
        state: Arc<std::sync::Mutex<super::Fake>>,
    },
}

#[cfg(any(test, feature = "test-util"))]
impl Drop for Lease {
    fn drop(&mut self) {
        if let Self::Fake { package, state } = self {
            state
                .lock()
                .unwrap_or_else(|_poisoned| std::process::abort())
                .owned_packages
                .remove(package);
        }
    }
}

fn busy(package: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("package already in use by another SpecGate command: {}", package.display()),
    )
}

impl CommandEnvironment {
    pub(crate) fn package_guard(&self, sources: impl AsRef<Path>) -> io::Result<PackageGuard> {
        let package = match &self.inner {
            Kind::Real => self.canonical(sources)?,
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => {
                if let Some(error) = state.lock().expect("fake system lock poisoned").package_failure.take() {
                    return Err(error);
                }
                sources.as_ref().to_path_buf()
            }
        };
        let lease = match &self.inner {
            Kind::Real => {
                let directory = std::env::temp_dir().join("specgate-package-locks");
                std::fs::create_dir_all(&directory)?;
                let identity = Sha256::digest(package.as_os_str().as_encoded_bytes());
                // Never unlink a lock file: another process may still hold its
                // inode. Closing the handle releases ownership, including on crash.
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(directory.join(format!("{identity:x}.lock")))?;
                match file.try_lock() {
                    Ok(()) => Lease::Real { _file: file },
                    Err(TryLockError::WouldBlock) => return Err(busy(&package)),
                    Err(TryLockError::Error(error)) => return Err(error),
                }
            }
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(state) => {
                if !state
                    .lock()
                    .expect("fake system lock poisoned")
                    .owned_packages
                    .insert(package.clone())
                {
                    return Err(busy(&package));
                }
                Lease::Fake {
                    package,
                    state: Arc::clone(state),
                }
            }
        };
        Ok(PackageGuard { _lease: Arc::new(lease) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn fake_ownership_lasts_until_the_last_clone_and_never_blocks_other_packages() {
        let system = CommandEnvironment::fake();
        let first = system.package_guard("candidate").unwrap();
        let clone = first.clone();
        let _other = system.package_guard("other").unwrap();
        assert_eq!(system.package_guard("candidate").unwrap_err().kind(), io::ErrorKind::WouldBlock);
        drop(first);
        assert_eq!(system.package_guard("candidate").unwrap_err().kind(), io::ErrorKind::WouldBlock);
        drop(clone);
        let _next = system.package_guard("candidate").unwrap();
    }

    #[test]
    fn acquisition_failures_are_reported() {
        let system = CommandEnvironment::fake();
        let Kind::Fake(state) = &system.inner else { unreachable!() };
        state.lock().unwrap().package_failure = Some(io::Error::other("cannot resolve package"));
        assert_eq!(system.package_guard("candidate").unwrap_err().to_string(), "cannot resolve package");
    }

    #[test]
    fn subprocess_observes_package_ownership() {
        let Some(package) = std::env::var_os("SPECGATE_LOCK_TEST_PACKAGE") else {
            return;
        };
        let expected = std::env::var("SPECGATE_LOCK_TEST_EXPECT").unwrap();
        let outcome = CommandEnvironment::real().package_guard(package);
        if expected == "busy" {
            assert_eq!(outcome.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        } else {
            let _guard = outcome.unwrap();
        }
    }

    #[test]
    fn real_ownership_rejects_another_process_and_releases_after_drop() {
        let package = tempfile::tempdir().unwrap();
        let system = CommandEnvironment::real();
        let run = |expected| {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "system::filesystem::package::tests::subprocess_observes_package_ownership",
                ])
                .env("SPECGATE_LOCK_TEST_PACKAGE", package.path().join("."))
                .env("SPECGATE_LOCK_TEST_EXPECT", expected)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        };
        let first = system.package_guard(package.path()).unwrap();
        run("busy");
        drop(first);
        run("free");
    }
}
