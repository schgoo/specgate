//! Candidate-rooted Cargo configuration resolution.

use super::failure;
use crate::Error;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct CargoConfig {
    inner: ConfigKind,
}

#[derive(Clone)]
enum ConfigKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Option<PathBuf>),
}

impl CargoConfig {
    pub(super) fn real() -> Self {
        Self { inner: ConfigKind::Real }
    }

    #[cfg(any(test, feature = "test-util"))]
    pub(super) fn fake(wrapper: Option<PathBuf>) -> Self {
        Self {
            inner: ConfigKind::Fake(wrapper),
        }
    }

    pub(crate) fn workspace_wrapper(&self, package: &Path) -> Result<Option<PathBuf>, Error> {
        match &self.inner {
            ConfigKind::Real => resolve(package, cargo_config2::ResolveOptions::default()),
            #[cfg(any(test, feature = "test-util"))]
            ConfigKind::Fake(wrapper) => Ok(wrapper.clone()),
        }
    }
}

fn resolve(package: &Path, options: cargo_config2::ResolveOptions) -> Result<Option<PathBuf>, Error> {
    cargo_config2::Config::load_with_options(package, options)
        .map(|config| config.build.rustc_workspace_wrapper)
        .map_err(|error| failure(format!("could not resolve candidate Cargo workspace wrapper: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn workspace_wrapper_respects_config_paths_and_environment_precedence() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".cargo")).unwrap();
        fs::create_dir_all(directory.path().join("candidate")).unwrap();
        fs::write(
            directory.path().join(".cargo").join("config.toml"),
            "[build]\nrustc-workspace-wrapper = 'tools/wrapper'\n",
        )
        .unwrap();
        let package = directory.path().join("candidate");
        let options = || cargo_config2::ResolveOptions::default().cargo_home(None);
        assert_eq!(
            resolve(&package, options().env([] as [(&str, &str); 0])).unwrap(),
            Some(directory.path().join("tools").join("wrapper"))
        );
        assert_eq!(
            resolve(&package, options().env([("RUSTC_WORKSPACE_WRAPPER", "user-wrapper")])).unwrap(),
            Some(PathBuf::from("user-wrapper"))
        );
        assert_eq!(resolve(&package, options().env([("RUSTC_WORKSPACE_WRAPPER", "")])).unwrap(), None);
        fs::write(directory.path().join(".cargo").join("config.toml"), "[build\n").unwrap();
        assert!(
            resolve(&package, options().env([] as [(&str, &str); 0]))
                .unwrap_err()
                .to_string()
                .contains("could not resolve candidate Cargo workspace wrapper")
        );
    }
}
