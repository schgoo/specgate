// Standalone, dependency-free compiler wrapper for CLI-owned test builds.
// ROOTS, FACADES, SCRATCH, and WRAPPER are supplied by the generated main source.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

#[cfg_attr(test, allow(dead_code, reason = "standalone compiler entry point is exercised through compile"))]
fn main() -> ExitCode {
    match compile(std::env::args_os().skip(1).collect(), ROOTS, FACADES, Path::new(SCRATCH), WRAPPER) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("SpecGate test-harness compiler: {error}");
            ExitCode::FAILURE
        }
    }
}

fn compile(
    mut arguments: Vec<OsString>,
    roots: &[&str],
    facades: &[&str],
    scratch: &Path,
    wrapper: Option<&str>,
) -> io::Result<ExitCode> {
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
            let cwd = std::env::current_dir()?;
            let source = cwd.join(&original);
            let root = TemporaryRoot::create(scratch)?;
            let dependencies = root.path.join("dependencies.d");
            let mut probe = dependency_arguments(&arguments);
            let mut emit = OsString::from("--emit=dep-info=");
            emit.push(&dependencies);
            probe.push(emit);
            let output = command(&compiler, wrapper).args(probe).output()?;
            if !output.status.success() {
                io::stdout().write_all(&output.stdout)?;
                io::stderr().write_all(&output.stderr)?;
                return Ok(exit_code(output.status));
            }
            let paths = dependency_paths(&fs::read_to_string(&dependencies)?)?;
            let mappings = path_mappings(&arguments)?;
            let mirror = root.path.join("sources");
            for dependency in paths.iter().map(|path| cwd.join(path)).chain([source.clone()]) {
                let copied = mirrored(&mirror, &dependency)?;
                // Paths such as empty/../module.rs still require empty/ to
                // exist, even though it contributes no compiler input file.
                for parent in dependency.ancestors().skip(1) {
                    fs::create_dir_all(mirrored(&mirror, parent)?)?;
                }
                fs::write(copied, fs::read(dependency)?)?;
            }
            let mut text = fs::read_to_string(&source)?;
            text.push('\n');
            text.push_str(&facade);
            text.push_str("::__specgate_test_harness!();\n");
            let copied = mirrored(&mirror, &source)?;
            fs::write(&copied, text)?;
            arguments[index] = copied.clone().into_os_string();
            // The sparse mirror keeps relative modules/includes intact, even
            // outside the package. Absolute includes still read original files.
            let mut remappings = Vec::new();
            for dependency in &paths {
                remap(
                    &mut remappings,
                    mirrored(&mirror, &cwd.join(dependency))?,
                    mapped_path(dependency, &mappings),
                );
            }
            remap(&mut remappings, copied, mapped_path(Path::new(&original), &mappings));
            // Keep large source sets below operating-system command-line limits.
            let response = root.path.join("remappings");
            let mut contents = String::new();
            for mapping in &remappings {
                let mapping = mapping
                    .to_str()
                    .filter(|mapping| !mapping.contains(['\r', '\n']))
                    .ok_or_else(|| io::Error::other("source path cannot be represented in a compiler argument file"))?;
                contents.push_str(mapping);
                contents.push('\n');
            }
            fs::write(&response, contents)?;
            let mut argument = OsString::from("@");
            argument.push(response);
            arguments.push(argument);
            Some(root)
        } else {
            None
        }
    } else {
        None
    };
    let status = command(&compiler, wrapper).args(arguments).status();
    if let Some(temporary) = temporary {
        temporary.remove()?;
    }
    Ok(exit_code(status?))
}

fn command(compiler: &OsString, wrapper: Option<&str>) -> Command {
    match wrapper {
        Some(wrapper) => {
            let mut command = Command::new(wrapper);
            command.arg(compiler);
            command
        }
        None => Command::new(compiler),
    }
}

fn exit_code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from)
}

fn dependency_arguments(arguments: &[OsString]) -> Vec<OsString> {
    let mut result = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--emit" || argument == "-o" {
            arguments.next();
        } else if !argument.to_str().is_some_and(|argument| argument.starts_with("--emit=")) {
            result.push(argument.clone());
        }
    }
    result
}

fn dependency_paths(document: &str) -> io::Result<Vec<PathBuf>> {
    let (_, dependencies) = document
        .lines()
        .find_map(|line| line.split_once(": "))
        .ok_or_else(|| io::Error::other("compiler did not report source dependencies"))?;
    let mut paths = Vec::new();
    let mut path = String::new();
    let mut characters = dependencies.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&' ') => {
                characters.next();
                path.push(' ');
            }
            ' ' if !path.is_empty() => paths.push(PathBuf::from(std::mem::take(&mut path))),
            ' ' => {}
            _ => path.push(character),
        }
    }
    if !path.is_empty() {
        paths.push(path.into());
    }
    if paths.is_empty() {
        return Err(io::Error::other("compiler reported no source dependencies"));
    }
    Ok(paths)
}

fn path_mappings(arguments: &[OsString]) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut mappings = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let value = if argument == "--remap-path-prefix" {
            arguments.next().and_then(|value| value.to_str())
        } else {
            argument.to_str().and_then(|value| value.strip_prefix("--remap-path-prefix="))
        };
        if let Some(value) = value {
            let (from, to) = value
                .rsplit_once('=')
                .ok_or_else(|| io::Error::other("invalid compiler path remapping"))?;
            mappings.push((from.into(), to.into()));
        }
    }
    Ok(mappings)
}

fn mapped_path(source: &Path, mappings: &[(PathBuf, PathBuf)]) -> PathBuf {
    for (from, to) in mappings.iter().rev() {
        if let Ok(rest) = source.strip_prefix(from) {
            return if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) };
        }
    }
    source.to_path_buf()
}

fn mirrored(directory: &Path, source: &Path) -> io::Result<PathBuf> {
    if !source.is_absolute() {
        return Err(io::Error::other("source dependency is not absolute"));
    }
    let mut result = directory.to_path_buf();
    let mut depth = 0;
    for component in source.components() {
        match component {
            Component::Prefix(prefix) => {
                let mut name = String::from("volume");
                for byte in prefix.as_os_str().as_encoded_bytes() {
                    use std::fmt::Write as _;
                    write!(name, "{byte:02x}").expect("writing volume identity");
                }
                result.push(name);
            }
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                result.push(name);
                depth += 1;
            }
            Component::ParentDir => {
                if depth > 0 {
                    result.pop();
                    depth -= 1;
                }
            }
        }
    }
    Ok(result)
}

fn remap(arguments: &mut Vec<OsString>, from: impl AsRef<std::ffi::OsStr>, to: impl AsRef<std::ffi::OsStr>) {
    let mut argument = OsString::from("--remap-path-prefix=");
    argument.push(from);
    argument.push("=");
    argument.push(to);
    arguments.push(argument);
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
}

impl TemporaryRoot {
    fn create(scratch: &Path) -> io::Result<Self> {
        fs::create_dir_all(scratch)?;
        for counter in 0..u32::MAX {
            let path = scratch.join(format!("root-{}-{counter}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("could not allocate a temporary test crate root"))
    }

    fn remove(self) -> io::Result<()> {
        fs::remove_dir_all(&self.path)
    }
}

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path)
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

    fn rustc() -> OsString {
        std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into())
    }

    #[test]
    fn dependency_paths_preserve_windows_separators_and_escaped_spaces() {
        assert_eq!(
            dependency_paths("C:\\scratch\\dependencies.d: src\\lib.rs src\\a\\ b.rs ../shared.rs\n\nsrc\\lib.rs:\n").unwrap(),
            [
                PathBuf::from("src\\lib.rs"),
                PathBuf::from("src\\a b.rs"),
                PathBuf::from("../shared.rs"),
            ]
        );
        dependency_paths("# env-dep:KEY=value\n").unwrap_err();
        dependency_paths("output: \n").unwrap_err();
    }

    #[test]
    fn scratch_roots_never_reuse_or_delete_interrupted_builds() {
        let directory = tempfile::tempdir().unwrap();
        let interrupted = TemporaryRoot::create(directory.path()).unwrap();
        let abandoned = interrupted.path.clone();
        fs::write(abandoned.join("in-progress.rs"), "owned by another build").unwrap();
        let _interrupted = std::mem::ManuallyDrop::new(interrupted);
        let next = TemporaryRoot::create(directory.path()).unwrap();
        assert_ne!(next.path, abandoned);
        next.remove().unwrap();
        assert_eq!(fs::read_to_string(abandoned.join("in-progress.rs")).unwrap(), "owned by another build");
    }

    #[test]
    fn interruption_worker() {
        let Some(directory) = std::env::var_os("SPECGATE_INTERRUPTION_TEST") else {
            return;
        };
        let directory = Path::new(&directory);
        let root = TemporaryRoot::create(&directory.join("scratch")).unwrap();
        fs::write(root.path.join("root.rs"), "interrupted compiler input").unwrap();
        fs::write(directory.join("ready"), root.path.to_str().unwrap()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    #[test]
    fn killed_wrapper_leaves_only_invocation_scratch() {
        let directory = tempfile::tempdir().unwrap();
        let sources = directory.path().join("sources");
        fs::create_dir(&sources).unwrap();
        fs::write(sources.join("root.rs"), "original source").unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "runner::harness::compiler::tests::interruption_worker"])
            .env("SPECGATE_INTERRUPTION_TEST", directory.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let ready = directory.path().join("ready");
        while !ready.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let abandoned = PathBuf::from(fs::read_to_string(ready).expect("child must allocate its root before termination"));
        assert!(abandoned.starts_with(directory.path().join("scratch")));
        assert!(abandoned.join("root.rs").is_file(), "termination must bypass the root destructor");
        assert_eq!(fs::read_dir(&sources).unwrap().count(), 1);
        assert_eq!(fs::read_to_string(sources.join("root.rs")).unwrap(), "original source");
        let next = TemporaryRoot::create(&directory.path().join("scratch")).unwrap();
        next.remove().unwrap();
        assert!(abandoned.is_dir(), "a later build must not delete another invocation's files");
    }

    #[test]
    fn read_only_roots_preserve_modules_includes_and_source_names() {
        let cwd = std::env::current_dir().unwrap();
        let directory = tempfile::tempdir_in(&cwd).unwrap();
        let sources = directory.path().join("sources");
        fs::create_dir_all(sources.join("nested")).unwrap();
        fs::create_dir(sources.join("empty")).unwrap();
        let source = sources.join("root.rs");
        let child = sources.join("child.rs");
        let custom = sources.join("nested").join("custom.rs");
        let included = sources.join("included.rs");
        let data = directory.path().join("data with spaces#$.txt");
        fs::write(&data, "value").unwrap();
        fs::write(&child, "pub fn value() -> i32 { assert_eq!(std::path::Path::new(file!()), std::path::Path::new(&std::env::var(\"EXPECTED_CHILD\").unwrap())); 2 }\n").unwrap();
        fs::write(&custom, "pub fn value() -> i32 { 3 }\n").unwrap();
        fs::write(&included, "pub fn included() -> i32 { 4 }\n").unwrap();
        fs::write(
            &source,
            r#"#![deny(warnings)]
#![doc = include_str!("../data with spaces#$.txt")]
mod child;
#[path = "empty/../nested/custom.rs"] mod custom;
include!("included.rs");
#[macro_export]
macro_rules! __specgate_test_harness {
    () => {
        #[test]
        fn generated_entry() {
            assert_eq!(std::path::Path::new(file!()), std::path::Path::new(&std::env::var("EXPECTED_ROOT").unwrap()));
            assert_eq!(include_str!("../data with spaces#$.txt"), "value");
            assert_eq!(child::value() + custom::value() + included(), 9);
        }
    };
}
"#,
        )
        .unwrap();
        let files = [&source, &child, &custom, &included, &data];
        let original = files.map(|file| (file, fs::read(file).unwrap(), fs::metadata(file).unwrap().permissions()));
        for (file, _, permissions) in &original {
            let mut readonly = permissions.clone();
            readonly.set_readonly(true);
            fs::set_permissions(file, readonly).unwrap();
        }
        #[cfg(unix)]
        let writable = {
            use std::os::unix::fs::PermissionsExt as _;
            let writable = fs::metadata(&sources).unwrap().permissions();
            fs::set_permissions(&sources, fs::Permissions::from_mode(0o555)).unwrap();
            writable
        };
        let mut results = Vec::new();
        for (index, (input, mapped)) in [
            (source.clone(), false),
            (source.strip_prefix(&cwd).unwrap().to_path_buf(), false),
            (source.clone(), true),
        ]
        .into_iter()
        .enumerate()
        {
            let executable = directory.path().join(format!("fixture-{index}{}", std::env::consts::EXE_SUFFIX));
            let mut arguments = vec![
                rustc(),
                "--test".into(),
                "--edition=2024".into(),
                "--crate-name=fixture".into(),
                input.clone().into_os_string(),
                "-o".into(),
                executable.clone().into_os_string(),
            ];
            let expected_root;
            let expected_child;
            if mapped {
                remap(&mut arguments, directory.path(), "mapped");
                expected_root = Path::new("mapped").join("sources").join("root.rs");
                expected_child = Path::new("mapped").join("sources").join("child.rs");
            } else {
                expected_root = input.clone();
                expected_child = input.parent().unwrap().join("child.rs");
            }
            let result = compile(
                arguments,
                &[source.to_str().unwrap()],
                &["crate"],
                &directory.path().join("scratch"),
                None,
            );
            results.push((result, executable, expected_root, expected_child));
        }
        #[cfg(unix)]
        fs::set_permissions(&sources, writable).unwrap();
        for (file, contents, permissions) in original {
            fs::set_permissions(file, permissions).unwrap();
            assert_eq!(fs::read(file).unwrap(), contents);
        }
        for (result, executable, expected_root, expected_child) in results {
            assert_eq!(result.unwrap(), ExitCode::SUCCESS);
            let status = Command::new(executable)
                .args(["--exact", "generated_entry"])
                .env("EXPECTED_ROOT", expected_root)
                .env("EXPECTED_CHILD", expected_child)
                .status()
                .unwrap();
            assert!(status.success());
        }
        assert_eq!(fs::read_dir(&sources).unwrap().count(), 5);
        assert_eq!(fs::read_dir(directory.path().join("scratch")).unwrap().count(), 0);
    }

    #[test]
    fn user_path_mappings_follow_compiler_delimiters_and_precedence() {
        let arguments = [
            "--remap-path-prefix",
            "path=with=equals=mapped",
            "--remap-path-prefix=path=with=equals=last",
        ]
        .map(OsString::from);
        let mappings = path_mappings(&arguments).unwrap();
        assert_eq!(
            mapped_path(&Path::new("path=with=equals").join("child.rs"), &mappings),
            Path::new("last").join("child.rs")
        );
        assert_eq!(mapped_path(Path::new("path=with=equals"), &mappings), Path::new("last"));
    }

    #[test]
    fn large_source_sets_fit_compiler_command_line_limits() {
        use std::fmt::Write as _;
        let directory = tempfile::tempdir().unwrap();
        let sources = directory.path().join("sources");
        let scratch = directory.path().join("scratch");
        fs::create_dir(&sources).unwrap();
        let source = sources.join("root.rs");
        let mut contents = String::from("#[macro_export] macro_rules! __specgate_test_harness { () => {} }\n");
        let mut remapping_bytes = 0;
        for index in 0..256 {
            let name = format!("resource-{index:03}-repeated-resource-name-for-large-argument-tests.txt");
            let path = sources.join(&name);
            fs::write(&path, "value").unwrap();
            writeln!(contents, "const _: &str = include_str!({name:?});").unwrap();
            remapping_bytes += mirrored(&scratch, &path).unwrap().as_os_str().len() + path.as_os_str().len();
        }
        assert!(remapping_bytes > 32_767, "the regression must exceed Windows' command-line limit");
        fs::write(&source, &contents).unwrap();
        let result = compile(
            vec![
                rustc(),
                "--test".into(),
                "--crate-name=fixture".into(),
                source.clone().into_os_string(),
                "-o".into(),
                directory
                    .path()
                    .join(format!("fixture{}", std::env::consts::EXE_SUFFIX))
                    .into_os_string(),
            ],
            &[source.to_str().unwrap()],
            &["crate"],
            &scratch,
            None,
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
        assert_eq!(fs::read_to_string(source).unwrap(), contents);
        assert_eq!(fs::read_dir(sources).unwrap().count(), 257);
        assert_eq!(fs::read_dir(scratch).unwrap().count(), 0);
    }

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
    fn compiler_spawn_failure_never_writes_to_sources() {
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
            &directory.path().join("scratch"),
            None,
        )
        .unwrap_err();
        assert_eq!(fs::read_to_string(&source).unwrap(), contents);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
        assert_eq!(fs::read_dir(directory.path().join("scratch")).unwrap().count(), 0);
    }

    #[test]
    fn compiler_failures_clean_scratch_without_changing_sources() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("root.rs");
        let scratch = directory.path().join("scratch");
        for contents in [
            "fn invalid(",
            "#[macro_export] macro_rules! __specgate_test_harness { () => { compile_error!(\"injected failure\"); } }",
        ] {
            fs::write(&source, contents).unwrap();
            let result = compile(
                vec![
                    rustc(),
                    "--test".into(),
                    "--crate-name=fixture".into(),
                    source.clone().into_os_string(),
                    "-o".into(),
                    directory.path().join("output").into_os_string(),
                ],
                &[source.to_str().unwrap()],
                &["crate"],
                &scratch,
                None,
            );
            assert_eq!(result.unwrap(), ExitCode::FAILURE);
            assert_eq!(fs::read_to_string(&source).unwrap(), contents);
            assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
        }
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }
}
