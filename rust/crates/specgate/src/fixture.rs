use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct FixtureProject {
    root: PathBuf,
}

impl FixtureProject {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn run(&self, enable_specgate: bool) -> Result<String, String> {
        let mut command = Command::new("cargo");
        command.current_dir(&self.root).arg("run").arg("--quiet");
        if enable_specgate {
            command.arg("--features").arg("specgate");
        }
        let output = command
            .output()
            .map_err(|error| format!("failed to run cargo in {}: {error}", self.root.display()))?;
        if !output.status.success() {
            return Err(format!(
                "fixture run failed with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        String::from_utf8(output.stdout).map_err(|error| error.to_string())
    }
}

pub fn create_runtime_fixture(
    case_name: &str,
    source: &str,
    driver: &str,
) -> Result<FixtureProject, String> {
    let root = runtime_fixture_root(case_name)?;
    if root.is_dir() {
        fs::remove_dir_all(&root)
            .map_err(|error| format!("failed to reset {}: {error}", root.display()))?;
    }
    fs::create_dir_all(root.join("src"))
        .map_err(|error| format!("failed to create fixture src: {error}"))?;

    fs::write(root.join("Cargo.toml"), cargo_toml()?)
        .map_err(|error| format!("failed to write fixture Cargo.toml: {error}"))?;
    fs::write(root.join("build.rs"), build_script())
        .map_err(|error| format!("failed to write fixture build.rs: {error}"))?;
    fs::write(root.join("src").join("lib.rs"), decorated_source(source))
        .map_err(|error| format!("failed to write fixture lib.rs: {error}"))?;
    fs::write(root.join("src").join("main.rs"), runtime_main(driver))
        .map_err(|error| format!("failed to write fixture main.rs: {error}"))?;

    Ok(FixtureProject { root })
}

fn runtime_fixture_root(case_name: &str) -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "failed to resolve workspace root".to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    Ok(workspace_root
        .join("target")
        .join("specgate-runtime-fixtures")
        .join(format!("{case_name}-{stamp}")))
}

fn cargo_toml() -> Result<String, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir
        .parent()
        .ok_or_else(|| "failed to resolve crates directory".to_string())?;
    let specgate = crates_dir.join("specgate");
    let annotations = crates_dir.join("specgate-annotations");
    Ok(format!(
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n[features]\nspecgate = []\n\n[dependencies]\nserde_json = \"1.0.145\"\nspecgate = {{ path = {:?} }}\nspecgate-annotations = {{ path = {:?} }}\n\n[build-dependencies]\nspecgate = {{ path = {:?} }}\n",
        specgate.display().to_string(),
        annotations.display().to_string(),
        specgate.display().to_string()
    ))
}

fn build_script() -> String {
    "use std::path::PathBuf;\n\nfn main() {\n    let manifest_dir = PathBuf::from(std::env::var(\"CARGO_MANIFEST_DIR\").expect(\"manifest dir\"));\n    let source_path = manifest_dir.join(\"src\").join(\"lib.rs\");\n    specgate::write_annotation_registry(&source_path, &manifest_dir, \"fixture\")\n        .expect(\"annotation registry should be written\");\n    println!(\"cargo:rerun-if-changed={}\", source_path.display());\n}\n"
        .to_string()
}

fn decorated_source(source: &str) -> String {
    format!(
        "#![allow(dead_code, unused_imports, unused_mut, unused_variables)]\nuse specgate_annotations::{{spec_capture, spec_checkpoint, spec_mock, spec_operation, spec_setup, SpecCapture}};\n\n{source}\n"
    )
}

fn runtime_main(driver: &str) -> String {
    format!(
        "#![allow(dead_code, unused_imports, unused_mut, unused_variables)]\nuse fixture::*;\n\nfn main() {{\n    specgate::runtime::reset();\n    {driver}\n    let traces = specgate::runtime::drain_traces();\n    print!(\"{{}}\", serde_json::to_string(&traces).expect(\"traces should serialize\"));\n}}\n"
    )
}
