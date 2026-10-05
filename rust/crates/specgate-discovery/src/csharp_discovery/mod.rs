//! C# reflection-based structural discovery.
//!
//! The Rust discovery path ([`crate::runner::run_discovery`]) links the
//! target crate and prints its link-time `discovery_json()`. The C# analog here
//! **builds the fixture's real project** into a per-run isolated artifacts tree
//! and then runs a small reflection program that loads the built fixture
//! assembly and **reflects** over its `[SpecOperation]` / `[SpecSetup]` /
//! `[SpecEvent]` / `[SpecException]` metadata (never scanning C# source text),
//! printing the same raw registry JSON shape the Rust runtime emits. Building
//! the real assembly - rather than compiling a source-globbed surrogate - means
//! operations that exist only in the compiled output (e.g. those emitted by a
//! build-time source generator) are discovered too. Types are normalized to
//! semantic CTSC-facing types with full reflection fidelity, including
//! `System.Reflection.NullabilityInfoContext` for nullable reference types - so
//! the parsed registry folds through the shared, language-neutral setup-folding
//! path in [`crate::schema`] to a `Schema` identical to the Rust
//! canonical.

use crate::error::{Error, ErrorKind};
use crate::runner::system::{ProcessRequest, System};
use std::path::{Path, PathBuf};

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::CSharp, message)
}
fn with_source(message: impl Into<String>, source: Error) -> Error {
    Error::cause(ErrorKind::CSharp, message, source)
}

// Debug is intentionally used for fixture discovery. The dotnet artifacts
// layout lowercases the same configuration name in its output directory.
const BUILD_CONFIGURATION: &str = "Debug";
const CONFIGURATION_DIR: &str = "debug";
// Preserve enough compiler context for actionable diagnostics without
// allowing verbose MSBuild output to dominate one discovery error.
const DIAGNOSTIC_LIMIT: usize = 40;

/// Paths produced by compiling one C# fixture.
pub(crate) struct BuildOutput {
    /// Compiled fixture assembly loaded by the generated runner.
    pub(crate) fixture_dll: PathBuf,
    /// Fixture output directory used to resolve runtime dependencies.
    pub(crate) fixture_out: PathBuf,
}

/// One batched C# reflection run: the requested documents plus the complete
/// operation-component inventory of the assembly that produced them.
#[derive(Debug)]
pub(crate) struct DiscoveryOutput {
    /// One raw registry document per requested component, in request order.
    pub(crate) documents: Vec<String>,
    /// Every component named by a `[SpecOperation]` in the compiled assembly,
    /// sorted and deduplicated - not just the requested ones.
    pub(crate) present_components: Vec<String>,
}

/// Build the fixture's real C# project into `scratch/artifacts` and return the
/// woven fixture assembly plus its copy-local output directory.
fn build_project(
    target: &crate::binding::Target,
    scratch: impl AsRef<Path>,
    context: impl AsRef<str>,
    system: &System,
) -> Result<BuildOutput, Error> {
    let scratch = scratch.as_ref();
    let context = context.as_ref();
    let pkg_abs = strip_prefix(
        system
            .filesystem
            .canonicalize(&target.package_root)
            .unwrap_or_else(|_| target.package_root.clone()),
    );
    system
        .filesystem
        .create_dir_all(scratch)
        .map_err(|source| with_source(format!("failed to scaffold C# {context} dir: {source}"), source))?;

    let csproj = find_project(&pkg_abs, system).ok_or_else(|| failure(format!("no .csproj found under {}", pkg_abs.display())))?;
    let project_name = csproj
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| failure("could not read fixture .csproj file name"))?
        .to_string();
    let csproj_text = system
        .filesystem
        .read_to_string(&csproj)
        .map_err(|source| with_source(format!("failed to read fixture .csproj: {source}"), source))?;
    let assembly_name = extract_tag(&csproj_text, "AssemblyName").unwrap_or_else(|| project_name.clone());

    let artifacts_dir = scratch.join("artifacts");
    let build = ProcessRequest::builder("dotnet")
        .arg("build")
        .arg(&csproj)
        .arg("-c")
        .arg(BUILD_CONFIGURATION)
        .arg("--artifacts-path")
        .arg(&artifacts_dir)
        .current_dir(scratch)
        .build();
    let build_out = system
        .process
        .output(&build)
        .map_err(|source| with_source(format!("failed to invoke dotnet build for C# {context}: {source}"), source))?;
    if !build_out.success {
        let stderr = String::from_utf8_lossy(&build_out.stderr);
        let stdout = String::from_utf8_lossy(&build_out.stdout);
        let combined = format!("{stderr}\n{stdout}");
        return Err(failure(format!(
            "C# {context} fixture build failed:\n{}",
            combined.lines().take(DIAGNOSTIC_LIMIT).collect::<Vec<_>>().join("\n")
        )));
    }

    let fixture_out = artifacts_dir.join("bin").join(&project_name).join(CONFIGURATION_DIR);
    let fixture_dll = fixture_out.join(format!("{assembly_name}.dll"));
    if !system.filesystem.exists(&fixture_dll) {
        return Err(failure(format!(
            "C# {context} build produced no assembly at {}",
            fixture_dll.display()
        )));
    }

    Ok(BuildOutput { fixture_dll, fixture_out })
}

/// Build the fixture once and reflect every requested `component` from the same
/// compiled assembly, returning one raw registry JSON document per component in
/// request order (each the same shape as the Rust runtime's `discovery_json()`:
/// `{ "operations": [...], "types": [...] }`) plus the assembly's complete
/// operation-component inventory.
///
/// A single fixture build and a single runner compilation serve every component,
/// so batch callers pay neither cost per component. Single-component discovery
/// requests a one-element batch, keeping one code path.
///
/// Scaffolds into an invocation-unique discovery scratch dir, so concurrent
/// discovery runs never clobber the same `Runner.dll`.
///
/// # Errors
///
/// Returns an error string when the scaffold, `dotnet` build/run, or output
/// read fails.
pub(crate) fn run_many<F: AsRef<str>, R: AsRef<str>>(
    target: &crate::binding::Target,
    first: F,
    rest: &[R],
    system: &System,
) -> Result<DiscoveryOutput, Error> {
    let components = std::iter::once(first.as_ref())
        .chain(rest.iter().map(AsRef::as_ref))
        .collect::<Vec<_>>();
    let settings = runner_settings(target, system);
    let sanitized_label: String = components
        .first()
        .copied()
        .unwrap_or("all")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let sanitized_framework: String = settings
        .framework
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let scratch = crate::runner::cache::InvocationCache::create_with(
        crate::runner::cache::CacheScope::new("csharp-discovery"),
        crate::runner::cache::CacheLabel::new(format!("{sanitized_label}_{sanitized_framework}_{}", components.len())),
        system,
    )?;
    run_in(target, &components, scratch.path(), system)
}

/// Build and run the C# reflection self-report for `target`/`components`,
/// scaffolding into `scratch`. Returns one raw registry JSON document per
/// requested component, in request order, plus the compiled assembly's
/// complete operation-component inventory.
///
/// # Errors
///
/// Returns an error string when the scaffold, `dotnet` build/run, or output
/// read fails.
fn run_in(
    target: &crate::binding::Target,
    components: &[impl AsRef<str>],
    scratch: impl AsRef<Path>,
    system: &System,
) -> Result<DiscoveryOutput, Error> {
    let scratch = scratch.as_ref();
    let settings = runner_settings(target, system);

    // 1. Build the fixture's REAL project into a per-run isolated artifacts tree,
    //    then reflect over the resulting assembly. This is what lets discovery
    //    observe operations that exist only in the compiled assembly (e.g. those
    //    emitted by a build-time source generator), which a source-globbing
    //    surrogate can never see. `--artifacts-path` lays every project in the
    //    build graph (the fixture + its referenced libraries) into its OWN
    //    `bin/<project>/<config>` and `obj/<project>/<config>` subfolders under
    //    the scratch dir, so concurrent discovery runs never contend on - nor even
    //    touch - the source project's `obj`/`bin`.
    let built = build_project(target, scratch, "discovery", system)?;
    let fixture_dll = built.fixture_dll;
    let fixture_out = built.fixture_out;
    // 2. Write a tiny reflection runner that references the same annotations
    //    assembly the fixture was compiled against, then reflects over the
    //    compiled fixture types.
    let runner_csproj = runner_project(&settings, &fixture_out, system);
    system
        .filesystem
        .write(scratch.join("Runner.csproj"), runner_csproj)
        .map_err(|source| with_source(format!("failed to write C# discovery Runner.csproj: {source}"), source))?;

    let program = render_program(components);
    system
        .filesystem
        .write(scratch.join("Program.cs"), program)
        .map_err(|source| with_source(format!("failed to write C# discovery Program.cs: {source}"), source))?;

    let out_dir = scratch.join("discovery");
    let _ = system.filesystem.remove_dir_all(&out_dir);
    system
        .filesystem
        .create_dir_all(&out_dir)
        .map_err(|source| with_source(format!("failed to create C# discovery output directory: {source}"), source))?;

    let request = ProcessRequest::builder("dotnet")
        .arg("run")
        .arg("--project")
        .arg(scratch.join("Runner.csproj"))
        .arg("--")
        .arg(&out_dir)
        .arg(&fixture_dll)
        .arg(&fixture_out)
        .current_dir(scratch)
        .build();
    let output = system
        .process
        .output(&request)
        .map_err(|source| with_source(format!("failed to invoke dotnet for C# discovery: {source}"), source))?;
    if !output.success {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{stderr}\n{stdout}");
        return Err(failure(format!(
            "C# discovery runner failed:\n{}",
            combined.lines().take(DIAGNOSTIC_LIMIT).collect::<Vec<_>>().join("\n")
        )));
    }

    let mut documents = Vec::with_capacity(components.len());
    for (index, component) in components.iter().enumerate() {
        let out_file = out_dir.join(format!("{index}.json"));
        let json = system.filesystem.read_to_string(&out_file).map_err(|source| {
            with_source(
                format!("C# discovery produced no output for component '{}': {source}", component.as_ref()),
                source,
            )
        })?;
        if json.trim().is_empty() {
            return Err(failure(format!(
                "C# discovery produced empty output for component '{}'",
                component.as_ref()
            )));
        }
        documents.push(json);
    }

    let inventory_file = out_dir.join("components.json");
    let inventory_json = system
        .filesystem
        .read_to_string(&inventory_file)
        .map_err(|source| with_source(format!("C# discovery produced no component inventory: {source}"), source))?;
    let present_components: Vec<String> = serde_json::from_str(&inventory_json).map_err(|source| {
        Error::cause(
            ErrorKind::CSharp,
            format!("C# discovery emitted an unreadable component inventory: {source}"),
            source,
        )
    })?;

    Ok(DiscoveryOutput {
        documents,
        present_components,
    })
}

fn runner_project(settings: &RunnerSettings, fixture_out: impl AsRef<Path>, system: &System) -> String {
    let fixture_out = fixture_out.as_ref();
    let lang_version = settings
        .lang_version
        .as_ref()
        .map(|value| format!("    <LangVersion>{}</LangVersion>\n", xml_text(value)))
        .unwrap_or_default();
    let output = xml_text(slash_path(fixture_out, system));
    format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    <TargetFramework>{framework}</TargetFramework>\n    \
         <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n    \
         <EnableNETAnalyzers>false</EnableNETAnalyzers>\n    <EnforceCodeStyleInBuild>false</EnforceCodeStyleInBuild>\n    \
         <TreatWarningsAsErrors>false</TreatWarningsAsErrors>\n    \
         <Nullable>{nullable}</Nullable>\n    <ImplicitUsings>{implicit_usings}</ImplicitUsings>\n{lang_version}  </PropertyGroup>\n  <ItemGroup>\n    \
         <Compile Include=\"Program.cs\" />\n  </ItemGroup>\n  <ItemGroup>\n    \
         <Reference Include=\"SpecGate.Annotations\">\n      <HintPath>{output}/SpecGate.Annotations.dll</HintPath>\n    </Reference>\n  </ItemGroup>\n</Project>\n",
        framework = xml_text(&settings.framework),
        nullable = xml_text(settings.nullable),
        implicit_usings = xml_text(settings.implicit_usings),
    )
}

/// Find the first `.csproj` directly under `package_root` (the fixture project).
fn find_project(package_root: impl AsRef<Path>, system: &System) -> Option<PathBuf> {
    let package_root = package_root.as_ref();
    system
        .filesystem
        .read_dir(package_root)
        .ok()?
        .into_iter()
        .filter_map(Result::ok)
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("csproj"))
}

mod program;
mod project;

use program::render_program;
#[cfg(test)]
use project::{ImplicitUsings, NullableMode};
use project::{RunnerSettings, extract_tag, runner_settings, slash_path, strip_prefix, xml_text};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::system::FailureKey;

    // End-to-end discovery tests below exercise generated runner requests,
    // registry parsing, component inventories, setup selection, and private
    // operation rejection through observed discovery results rather than
    // matching implementation text in the generated C# source.

    #[test]
    fn project_escaping() {
        let settings = RunnerSettings {
            framework: "net10.0".to_string(),
            nullable: NullableMode::Enable,
            implicit_usings: ImplicitUsings::Enable,
            lang_version: None,
        };
        let project = runner_project(&settings, Path::new("build&a"), &System::real());
        assert!(project.contains("build&amp;a"));
        assert!(!project.contains("build&a/SpecGate.Annotations.dll"));
    }

    fn fake_target() -> crate::binding::Target {
        crate::binding::Target {
            package_root: PathBuf::from("fixture"),
            runtime: crate::binding::Runtime::Smol,
            framework: None,
            command: None,
            outputs: None,
        }
    }

    fn fake_system(results: Vec<Result<crate::runner::system::ProcessOutput, String>>) -> System {
        use crate::runner::system::{FakeFs, FakeProcess};
        use std::collections::BTreeMap;
        let scratch = PathBuf::from("scratch");
        let fixture_dll = scratch.join("artifacts/bin/Fixture/debug/Fixture.dll");
        let out = scratch.join("discovery");
        let filesystem = FakeFs::builder()
            .files(BTreeMap::from([
                (
                    PathBuf::from("fixture/Fixture.csproj"),
                    b"<Project><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>".to_vec(),
                ),
                (fixture_dll, Vec::new()),
                (out.join("0.json"), br#"{"operations":[],"types":[]}"#.to_vec()),
                (out.join("components.json"), br#"["fixture.component"]"#.to_vec()),
            ]))
            .directories(BTreeMap::from([(
                PathBuf::from("fixture"),
                vec![PathBuf::from("fixture/Fixture.csproj")],
            )]))
            .build();
        System::builder((filesystem, FakeProcess::queued(results)))
            .current_directory(Ok(PathBuf::from("injected-cwd")))
            .process_id(99)
            .build()
    }

    fn success() -> crate::runner::system::ProcessOutput {
        crate::runner::system::ProcessOutput {
            success: true,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn runner_failures() {
        let build_spawn = fake_system(vec![Err("spawn denied".to_string())]);
        assert_eq!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &build_spawn).unwrap_err(),
            "failed to invoke dotnet build for C# discovery: spawn denied"
        );

        let build_nonzero = fake_system(vec![Ok(crate::runner::system::ProcessOutput {
            success: false,
            stdout: b"stdout line".to_vec(),
            stderr: b"stderr line".to_vec(),
        })]);
        let error = run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &build_nonzero).unwrap_err();
        assert!(error.contains("C# discovery fixture build failed:\nstderr line\nstdout line"));

        for (result, expected) in [
            (
                Err("run denied".to_string()),
                "failed to invoke dotnet for C# discovery: run denied",
            ),
            (
                Ok(crate::runner::system::ProcessOutput {
                    success: false,
                    stdout: b"runner stdout".to_vec(),
                    stderr: b"runner stderr".to_vec(),
                }),
                "C# discovery runner failed:\nrunner stderr\nrunner stdout",
            ),
        ] {
            let system = fake_system(vec![Ok(success()), result]);
            assert!(
                run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system)
                    .unwrap_err()
                    .contains(expected)
            );
        }
    }

    #[test]
    fn invalid_runner_outputs() {
        let system = fake_system(vec![Ok(success()), Ok(success())]);
        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .remove_file(PathBuf::from("scratch/discovery/0.json"));
        assert!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system)
                .unwrap_err()
                .contains("produced no output for component 'fixture.component'")
        );

        let system = fake_system(vec![Ok(success()), Ok(success())]);
        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .insert_file(PathBuf::from("scratch/discovery/0.json"), b"  ".to_vec());
        assert_eq!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system).unwrap_err(),
            "C# discovery produced empty output for component 'fixture.component'"
        );

        let system = fake_system(vec![Ok(success()), Ok(success())]);
        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .insert_file(PathBuf::from("scratch/discovery/components.json"), b"{".to_vec());
        assert!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system)
                .unwrap_err()
                .contains("unreadable component inventory")
        );
    }

    #[test]
    fn scaffold_failures() {
        for (key, expected) in [
            ("create_dir_all", "failed to scaffold C# discovery dir"),
            ("write:Runner.csproj", "failed to write C# discovery Runner.csproj"),
            ("write:Program.cs", "failed to write C# discovery Program.cs"),
        ] {
            let system = fake_system(vec![Ok(success()), Ok(success())]);
            system.filesystem.fake_state().borrow_mut().fail(FailureKey::test(key), "denied");
            assert!(
                run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system)
                    .unwrap_err()
                    .contains(expected)
            );
        }
        let system = fake_system(vec![Ok(success())]);
        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .remove_file(PathBuf::from("scratch/artifacts/bin/Fixture/debug/Fixture.dll"));
        assert!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system)
                .unwrap_err()
                .contains("build produced no assembly at")
        );

        let system = fake_system(Vec::new());
        assert_eq!(slash_path(Path::new("relative/path"), &system), "injected-cwd/relative/path");
        let failed_cwd = System::builder((
            crate::runner::system::FakeFs::default(),
            crate::runner::system::FakeProcess::default(),
        ))
        .current_directory(Err("cwd denied".to_string()))
        .process_id(1)
        .build();
        assert_eq!(slash_path(Path::new("relative/path"), &failed_cwd), "relative/path");
    }

    #[test]
    fn fixture_project_selection() {
        use crate::runner::system::{FailureKey, FakeFs, FakeProcess};
        use std::collections::BTreeMap;
        let usable = PathBuf::from("fixture/Usable.csproj");
        let system = System::builder((
            FakeFs::builder()
                .directory_results(BTreeMap::from([(
                    PathBuf::from("fixture"),
                    vec![
                        Err("entry denied".to_string()),
                        Ok(PathBuf::from("fixture/readme.txt")),
                        Ok(usable.clone()),
                    ],
                )]))
                .build(),
            FakeProcess::default(),
        ))
        .current_directory(Ok(PathBuf::from("cwd")))
        .process_id(1)
        .build();
        assert_eq!(find_project(Path::new("fixture"), &system), Some(usable));

        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .fail(FailureKey::test("read_dir:fixture"), "root denied");
        assert_eq!(find_project(Path::new("fixture"), &system), None);
    }

    #[test]
    fn diagnostic_truncation() {
        use std::fmt::Write as _;
        let stderr = (0..45).fold(String::new(), |mut diagnostics, index| {
            writeln!(diagnostics, "error {index}").unwrap();
            diagnostics
        });
        let system = fake_system(vec![Ok(crate::runner::system::ProcessOutput {
            success: false,
            stdout: b"stdout omitted\n".to_vec(),
            stderr: stderr.into_bytes(),
        })]);
        let error = run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system).unwrap_err();
        let diagnostics = error.strip_prefix("C# discovery fixture build failed:\n").unwrap();
        assert_eq!(diagnostics.lines().count(), DIAGNOSTIC_LIMIT);
        assert!(diagnostics.ends_with("error 39"));
        assert!(!diagnostics.contains("error 40"));
        assert!(!diagnostics.contains("stdout omitted"));
    }

    #[test]
    fn output_directory_failure() {
        let system = fake_system(vec![Ok(success())]);
        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .fail(FailureKey::test("create_dir_all:discovery"), "output denied");
        assert_eq!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system).unwrap_err(),
            "failed to create C# discovery output directory: output denied"
        );
        let state = system.filesystem.fake_state();
        let state = state.borrow();
        assert!(state.has_directory(Path::new("scratch")));
        assert!(state.has_file(Path::new("scratch/Runner.csproj")));
        assert!(state.has_file(Path::new("scratch/Program.cs")));
        assert_eq!(system.process.fake_state().lock().unwrap().requests.len(), 1);
    }

    #[test]
    fn missing_component_inventory() {
        let system = fake_system(vec![Ok(success()), Ok(success())]);
        system
            .filesystem
            .fake_state()
            .borrow_mut()
            .remove_file(PathBuf::from("scratch/discovery/components.json"));
        assert!(
            run_in(&fake_target(), &["fixture.component"], Path::new("scratch"), &system)
                .unwrap_err()
                .contains("C# discovery produced no component inventory")
        );
    }

    #[test]
    fn slash_path_normalization() {
        let system = fake_system(Vec::new());
        #[cfg(windows)]
        let absolute = Path::new(r"C:\work\absolute.dll");
        #[cfg(not(windows))]
        let absolute = Path::new("/work/absolute.dll");
        assert_eq!(slash_path(absolute, &system), absolute.display().to_string().replace('\\', "/"));
        #[cfg(windows)]
        assert_eq!(slash_path(Path::new(r"\\?\C:\work\fixture.dll"), &system), "C:/work/fixture.dll");
    }
}
