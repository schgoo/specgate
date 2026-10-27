//! Real Cargo evidence for component hooks, freshness, and ordinary build isolation.

use super::*;
use std::fs;
use std::process::{Command, Output};

fn succeeded(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

struct ReadOnlySources(Vec<(PathBuf, fs::Permissions)>);

impl ReadOnlySources {
    fn new(paths: &[PathBuf]) -> Self {
        let mut guard = Self(Vec::new());
        for path in paths {
            let original = fs::metadata(path).unwrap().permissions();
            guard.0.push((path.clone(), original.clone()));
            let mut permissions = original;
            permissions.set_readonly(true);
            fs::set_permissions(path, permissions).unwrap();
        }
        guard
    }
}

impl Drop for ReadOnlySources {
    fn drop(&mut self) {
        for (path, permissions) in self.0.iter().rev() {
            fs::set_permissions(path, permissions.clone()).unwrap();
        }
    }
}

#[test]
fn component_hook_tracks_entries_and_dispatch_without_source_injection() {
    for crate_types in [&["rlib"][..], &["rlib", "cdylib"][..]] {
        check_component_hook(crate_types);
    }
}

fn check_component_hook(crate_types: &[&str]) {
    let cache = tempfile::tempdir().unwrap();
    let package = cache.path().join("candidate");
    let scratch = cache.path().join("generated");
    let source = package.join("src");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(package.join(".cargo")).unwrap();
    fs::create_dir_all(&scratch).unwrap();
    let facade = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("specgate");
    let manifest = serde_json::json!({
        "package": {"name": "component-hook-fixture", "version": "0.0.0", "edition": "2024"},
        "lib": {"crate-type": crate_types},
        "dependencies": {"sg": {"package": "specgate", "path": facade}},
        "workspace": {},
    });
    fs::write(package.join("Cargo.toml"), toml::to_string(&manifest).unwrap()).unwrap();
    let lib = source.join("lib.rs");
    let contents = "#![deny(warnings)]\n\
        #![doc = include_str!(\"value.txt\")]\n\
        sg::spec_component!(\"fixture.unused_hook_default\");\n\
        #[path = \"support.rs\"] mod support;\n\
        #[sg::spec_operation(\"value\", spec = \"fixture.component_hook\")]\n\
        pub fn value() -> i32 {\n\
            assert!(file!().ends_with(\"src/lib.rs\") || file!().ends_with(\"src\\\\lib.rs\"));\n\
            support::value()\n\
        }\n\
        #[test] fn ordinary() {\n\
            assert!(std::env::var_os(\"SPECGATE_TEST_ENTRY\").is_none());\n\
            assert_eq!(value(), 7);\n\
        }\n";
    fs::write(&lib, contents).unwrap();
    fs::write(
        source.join("support.rs"),
        "pub fn value() -> i32 { include_str!(\"value.txt\").trim().parse().unwrap() }\n",
    )
    .unwrap();
    fs::write(source.join("value.txt"), "7\n").unwrap();

    let wrapper_source = cache.path().join("wrapper.rs");
    let wrapper = cache.path().join(format!("wrapper{}", std::env::consts::EXE_SUFFIX));
    let log = cache.path().join("wrapper.log");
    fs::write(
        &wrapper_source,
        format!(
            "use std::io::Write;\nfn main() {{\n\
             let mut args = std::env::args_os().skip(1);\n\
             let compiler = args.next().unwrap();\n\
             let args: Vec<_> = args.collect();\n\
             if args.iter().any(|arg| arg == \"--test\") {{\n\
                 let mut log = std::fs::OpenOptions::new().create(true).append(true).open({log:?}).unwrap();\n\
                 writeln!(log, \"{{}}\", compiler.to_string_lossy()).unwrap();\n\
             }}\n\
             std::process::exit(std::process::Command::new(compiler).args(args).status().unwrap().code().unwrap_or(1));\n\
             }}\n"
        ),
    )
    .unwrap();
    succeeded(Command::new("rustc").arg(&wrapper_source).arg("-o").arg(&wrapper).output().unwrap());
    let config = serde_json::json!({
        "build": {"rustc-wrapper": wrapper, "rustc-workspace-wrapper": wrapper}
    });
    fs::write(package.join(".cargo").join("config.toml"), toml::to_string(&config).unwrap()).unwrap();

    let ordinary_build = |build_directory: Option<&Path>| {
        let mut command = Command::new(crate::runner::cargo_bin());
        if let Some(directory) = build_directory {
            command.env("CARGO_BUILD_BUILD_DIR", directory);
        }
        succeeded(
            command
                .args(["test", "--no-run", "--quiet", "--message-format=json"])
                .current_dir(&package)
                .env_remove("CARGO")
                .env_remove("CARGO_MANIFEST_DIR")
                .env_remove("SPECGATE_TEST_ENTRY")
                .env_remove("SPECGATE_TEST_DISPATCH")
                .env("CARGO_INCREMENTAL", "0")
                .env("CARGO_TARGET_DIR", scratch.join("target"))
                .output()
                .unwrap(),
        )
    };
    let ordinary = ordinary_build(None);
    let artifacts: Vec<Value> = String::from_utf8_lossy(&ordinary.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact" && message["target"]["name"] == "component_hook_fixture")
        .collect();
    let binary = artifacts
        .iter()
        .find_map(|message| message["executable"].as_str())
        .expect("ordinary test binary");
    let files: BTreeMap<PathBuf, Vec<u8>> = artifacts
        .iter()
        .flat_map(|artifact| artifact["filenames"].as_array().unwrap())
        .map(|file| {
            let path = PathBuf::from(file.as_str().unwrap());
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    if crate_types.contains(&"cdylib") {
        assert!(
            files
                .keys()
                .any(|path| path.extension().is_some_and(|ext| ext == std::env::consts::DLL_EXTENSION))
        );
    }
    let listed = succeeded(Command::new(binary).args(["--list", "--format", "terse"]).output().unwrap());
    assert_eq!(String::from_utf8(listed.stdout).unwrap(), "ordinary: test\n");
    succeeded(Command::new(binary).arg("--exact").arg("ordinary").output().unwrap());

    let readonly_paths = vec![lib.clone(), source.join("support.rs"), source.join("value.txt")];
    #[cfg(unix)]
    let readonly_paths = [readonly_paths, vec![source.clone()]].concat();
    let _readonly = ReadOnlySources::new(&readonly_paths);
    let entry = scratch.join("entry.rs");
    let next_entry = scratch.join("next-entry.rs");
    let dispatch = scratch.join("dispatch.rs");
    let next_dispatch = scratch.join("next-dispatch.rs");
    let metadata = scratch.join("metadata.json");
    let module_dispatch = scratch.join("module.rs");
    fs::write(
        &module_dispatch,
        "{ macro_rules! __specgate_dispatch { ($($unused:tt)*) => { None::<__specgate::harness::Invocation> }; } __specgate_selected!() }",
    )
    .unwrap();
    let modules = BTreeMap::from([("component_hook_fixture".to_string(), module_dispatch)]);
    for (path, marker) in [(&entry, 1), (&next_entry, 2)] {
        fs::write(
            path,
            format!(
                "{{ assert_eq!(crate::value(), 7); let value: i32 = include!(env!(\"SPECGATE_TEST_DISPATCH\")); \
                 std::fs::write(std::env::var_os(\"SPECGATE_TEST_METADATA\").unwrap(), \
                 format!(\"{marker}\\n{{value}}\\n{{}}\", __specgate::discovery())).unwrap(); }}"
            ),
        )
        .unwrap();
    }
    fs::write(&dispatch, "10").unwrap();
    fs::write(&next_dispatch, "20").unwrap();
    let system = System::real();
    for (entry_path, dispatch_path, marker, value) in [
        (&entry, &dispatch, 1, 10),
        (&next_entry, &dispatch, 2, 10),
        (&next_entry, &next_dispatch, 2, 20),
        (&next_entry, &next_dispatch, 2, 30),
    ] {
        if value == 30 {
            fs::write(dispatch_path, "30").unwrap();
        }
        let log_before = fs::read_to_string(&log).unwrap().lines().count();
        let binaries = build_in(&package, entry_path, Some(dispatch_path), &modules, &scratch, None, &system).unwrap();
        assert_eq!(binaries.len(), 1);
        let log_after = fs::read_to_string(&log).unwrap().lines().count();
        assert!(
            log_after >= log_before + 2,
            "both configured compiler wrappers must handle generated test builds"
        );
        let binary = &binaries[0];
        succeeded(
            Command::new(&binary.executable)
                .arg("--exact")
                .arg(&binary.entry)
                .env("SPECGATE_TEST_METADATA", &metadata)
                .output()
                .unwrap(),
        );
        let text = fs::read_to_string(&metadata).unwrap();
        let mut parts = text.splitn(3, '\n');
        assert_eq!(parts.next().unwrap(), marker.to_string());
        assert_eq!(parts.next().unwrap(), value.to_string());
        let registry: Value = serde_json::from_str(parts.next().unwrap()).unwrap();
        assert!(!registry.to_string().contains("fixture.unused_hook_default"));
        assert_eq!(registry["operations"][0]["component"], "fixture.component_hook");
        assert_eq!(registry["operations"][0]["name"], "value");
        for (path, bytes) in &files {
            assert_eq!(&fs::read(path).unwrap(), bytes, "generated builds overwrote {}", path.display());
        }
    }
    assert_eq!(fs::read_to_string(&lib).unwrap(), contents);
    assert_eq!(fs::read_dir(&source).unwrap().count(), 3);
    let generated_directory = super::super::super::metadata::harness_build_directory(&package, &scratch.join("target"), &system).unwrap();
    let rebuilt = ordinary_build(Some(&generated_directory));
    let rebuilt_binary = String::from_utf8_lossy(&rebuilt.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|artifact| artifact["executable"].as_str().map(str::to_string))
        .unwrap();
    let listed = succeeded(
        Command::new(&rebuilt_binary)
            .args(["--list", "--format", "terse"])
            .output()
            .unwrap(),
    );
    assert_eq!(String::from_utf8(listed.stdout).unwrap(), "ordinary: test\n");
    let listed = succeeded(Command::new(binary).args(["--list", "--format", "terse"]).output().unwrap());
    assert_eq!(String::from_utf8(listed.stdout).unwrap(), "ordinary: test\n");
}

#[test]
fn generated_test_targets_require_local_components_even_with_explicit_specs() {
    let cache = tempfile::tempdir().unwrap();
    let package = cache.path().join("candidate");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::create_dir_all(package.join("tests")).unwrap();
    let facade = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("specgate");
    let manifest = serde_json::json!({
        "package": {"name": "missing-component-fixture", "version": "0.0.0", "edition": "2024"},
        "dependencies": {"sg": {"package": "specgate", "path": facade}},
        "workspace": {},
    });
    fs::write(package.join("Cargo.toml"), toml::to_string(&manifest).unwrap()).unwrap();
    let entry = cache.path().join("entry.rs");
    fs::write(&entry, "{}").unwrap();
    let operation = "#[sg::spec_operation(\"value\", spec = \"fixture.explicit\")]\npub fn value() -> i32 { 7 }\n";
    let setup = "#[sg::spec_setup(\"value\", spec = \"fixture.explicit\", fills = \"initial\")]\nfn initial() -> i32 { 7 }\n";
    for (target, path, source) in [
        ("--lib", package.join("src").join("lib.rs"), operation.to_string()),
        (
            "--bins",
            package.join("src").join("main.rs"),
            format!("{operation}\nfn main() {{}}\n"),
        ),
        ("--tests", package.join("tests").join("local.rs"), setup.to_string()),
    ] {
        fs::write(package.join("src").join("lib.rs"), "sg::spec_component!(\"fixture.library\");\n").unwrap();
        fs::write(&path, source).unwrap();
        let build = |generated| {
            let mut command = Command::new(crate::runner::cargo_bin());
            command
                .args(["test", target, "--no-run", "--quiet"])
                .current_dir(&package)
                .env_remove("CARGO")
                .env_remove("CARGO_MANIFEST_DIR")
                .env_remove("SPECGATE_TEST_DISPATCH")
                .env("CARGO_INCREMENTAL", "0")
                .env("CARGO_TARGET_DIR", cache.path().join("target"));
            if generated {
                command.env("SPECGATE_TEST_ENTRY", &entry);
            } else {
                command.env_remove("SPECGATE_TEST_ENTRY");
            }
            command.output().unwrap()
        };
        succeeded(build(false));
        let rejected = build(true);
        assert!(
            !rejected.status.success(),
            "generated {target} build accepted a missing declaration"
        );
        let diagnostics = String::from_utf8_lossy(&rejected.stderr);
        assert!(
            diagnostics.contains("__specgate_requires_crate_root_spec_component"),
            "{diagnostics}"
        );
        if path.file_name().unwrap() != "lib.rs" {
            fs::remove_file(path).unwrap();
        }
    }
}
