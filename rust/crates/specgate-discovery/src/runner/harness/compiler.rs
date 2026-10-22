// Standalone, dependency-free compiler wrapper for CLI-owned test builds.
// ROOTS and FACADES are supplied by the generated crate's main source.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[cfg_attr(test, allow(dead_code, reason = "standalone compiler entry point is exercised through compile"))]
fn main() -> ExitCode {
    match compile(std::env::args_os().skip(1).collect(), ROOTS, FACADES) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("SpecGate test-harness compiler: {error}");
            ExitCode::FAILURE
        }
    }
}

fn compile(mut arguments: Vec<OsString>, roots: &[&str], facades: &[&str]) -> io::Result<ExitCode> {
    if arguments.is_empty() {
        return Err(io::Error::other("Cargo did not supply a compiler executable"));
    }
    let compiler = arguments.remove(0);
    let source = if arguments.iter().any(|argument| argument == "--test") {
        arguments.iter().enumerate().find_map(|(index, argument)| {
            let path = fs::canonicalize(argument).ok()?;
            roots
                .iter()
                .any(|root| fs::canonicalize(root).is_ok_and(|root| root == path))
                .then_some(index)
        })
    } else {
        None
    };
    let temporary = if let Some(index) = source {
        let facade = selected_facade(&arguments, facades)?;
        if let Some(facade) = facade {
            let original = arguments[index].clone();
            let source = std::env::current_dir()?.join(&original);
            let mut root = TemporaryRoot::create(&source)?;
            let mut text = fs::read_to_string(&source)?;
            text.push('\n');
            text.push_str(&facade);
            text.push_str("::__specgate_test_harness!();\n");
            root.file.write_all(text.as_bytes())?;
            root.file.flush()?;
            arguments[index] = root.path.clone().into_os_string();
            // Preserve file!(), diagnostics, and source-relative paths while the
            // sibling root keeps inner attributes and module resolution intact.
            let mut remap = OsString::from("--remap-path-prefix=");
            remap.push(&root.path);
            remap.push("=");
            remap.push(original);
            arguments.push(remap);
            Some(root)
        } else {
            None
        }
    } else {
        None
    };
    let status = Command::new(compiler).args(arguments).status();
    if let Some(temporary) = temporary {
        temporary.remove()?;
    }
    let status = status?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

fn selected_facade(arguments: &[OsString], facades: &[&str]) -> io::Result<Option<String>> {
    let externs = arguments
        .windows(2)
        .filter_map(|pair| (pair[0] == "--extern").then(|| pair[1].to_str()?.split('=').next()).flatten());
    let aliases: Vec<_> = externs.collect();
    let selected: Vec<_> = facades
        .iter()
        .filter(|alias| **alias == "crate" || aliases.contains(alias))
        .collect();
    match selected.as_slice() {
        [] => Ok(None),
        [alias] if **alias == "crate" => Ok(Some("crate".to_string())),
        [alias] => Ok(Some(format!("::r#{alias}"))),
        _ => Err(io::Error::other(
            "multiple SpecGate facade dependencies are active in one test target",
        )),
    }
}

struct TemporaryRoot {
    path: PathBuf,
    file: fs::File,
}

impl TemporaryRoot {
    fn create(source: &Path) -> io::Result<Self> {
        let parent = source.parent().ok_or_else(|| io::Error::other("test crate root has no parent"))?;
        for counter in 0..u32::MAX {
            // Not a .rs file or directory: Cargo cannot discover a new binary,
            // integration-test, example, or benchmark target while this exists.
            let path = parent.join(format!(".specgate-root-{}-{counter}", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok(Self { path, file }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("could not allocate a temporary test crate root"))
    }

    fn remove(self) -> io::Result<()> {
        fs::remove_file(&self.path)
    }
}

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "SpecGate could not remove temporary test crate root {}: {error}",
                self.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_selection_uses_only_active_compiler_aliases() {
        let arguments = ["--extern", "renamed=specgate.rlib", "--extern", "other=other.rlib"].map(OsString::from);
        assert_eq!(
            selected_facade(&arguments, &["renamed", "inactive"]).unwrap(),
            Some("::r#renamed".into())
        );
        assert_eq!(selected_facade(&arguments, &["inactive"]).unwrap(), None);
        assert_eq!(selected_facade(&[], &["crate"]).unwrap(), Some("crate".into()));
        selected_facade(&arguments, &["renamed", "other"]).unwrap_err();
    }

    #[test]
    fn compiler_spawn_failure_removes_only_the_generated_sibling() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("test.rs");
        let contents = "#![deny(warnings)]\n";
        fs::write(&source, contents).unwrap();
        let missing = directory.path().join("missing-compiler");
        compile(
            vec![
                missing.into_os_string(),
                "--test".into(),
                source.clone().into_os_string(),
                "--extern".into(),
                "renamed=specgate.rlib".into(),
            ],
            &[source.to_str().unwrap()],
            &["renamed"],
        )
        .unwrap_err();
        assert_eq!(fs::read_to_string(&source).unwrap(), contents);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
