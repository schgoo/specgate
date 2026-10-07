//! Focused discovery normalization, parsing, and runner tests.

use super::runner::discovery_cargo;
use super::*;
use crate::identity::TypeName;
use std::path::PathBuf;

fn registry(json: impl AsRef<str>) -> Registry {
    Registry::parse(json).unwrap()
}

#[test]
fn scoped_setups() {
    let registry = registry(
        r#"{"operations":[
                {"name":"run","module_path":"fixture","fn_name":"run","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"i32","fills":"","params":[["value","i32"]],"component":"component.a"},
                {"name":"run","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"State","fills":"","params":[["leaked","string"]],"component":"component.b"}
            ],"types":[]}"#,
    );
    assert_eq!(
        raw_inputs(&registry.ops[0], &registry).unwrap(),
        vec![crate::setup::SetupInput {
            name: "value".into(),
            source_type: "i32".into(),
        }]
    );
}

#[test]
fn ambiguous_setups() {
    let registry = registry(
        r#"{"operations":[
                {"name":"run","module_path":"fixture","fn_name":"run","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"i32","fills":"","params":[["left","State"],["right","State"]],"component":"component.a"},
                {"name":"run","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"State","fills":"","params":[],"component":"component.a"}
            ],"types":[]}"#,
    );
    assert!(raw_inputs(&registry.ops[0], &registry).unwrap_err().contains("ambiguously matches"));
}

#[test]
fn surface_validation() {
    let duplicate = registry(
        r#"{"operations":[
                {"name":"render","module_path":"fixture","fn_name":"render_one","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"String","fills":"","params":[],"component":"fixture.duplicate"},
                {"name":"render","module_path":"fixture","fn_name":"render_two","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"String","fills":"","params":[],"component":"fixture.duplicate"}
            ],"types":[]}"#,
    );
    assert_eq!(
        normalize_registry(&duplicate, crate::binding::Language::Rust, "fixture.duplicate").unwrap_err(),
        "operation 'fixture.duplicate::render' is declared 2 times; operation identity must be unique within a component"
    );

    let private = registry(
        r#"{"operations":[
                {"name":"secret","module_path":"fixture","fn_name":"secret","is_setup":false,"is_async":false,"is_method":false,"is_public":false,"return_type":"i32","fills":"","params":[],"component":"fixture.private"}
            ],"types":[]}"#,
    );
    assert_eq!(
        normalize_registry(&private, crate::binding::Language::Rust, "fixture.private").unwrap_err(),
        "operation 'fixture.private::secret' is declared on private function 'secret'; discovery exposes only public operations"
    );

    let orphan = registry(
        r#"{"operations":[
                {"name":"increment","module_path":"fixture","fn_name":"make_counter","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[],"component":"fixture.orphan"}
            ],"types":[{"name":"Counter","module_path":"fixture","kind":"struct","component":"fixture.orphan","fields":[["count","i32"]],"variants":[]}]}"#,
    );
    assert_eq!(
        normalize_registry(&orphan, crate::binding::Language::Rust, "fixture.orphan").unwrap_err(),
        "setup 'make_counter' for 'fixture.orphan::increment' has no operation to construct; annotate the operation or remove the setup"
    );
}

#[test]
fn invalid_surfaces() {
    let method = registry(
        r#"{"operations":[
                {"name":"increment","module_path":"fixture","fn_name":"increment","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.method"}
            ],"types":[{"name":"Counter","module_path":"fixture","kind":"struct","component":"fixture.method","fields":[["count","i32"]],"variants":[]}]}"#,
    );
    assert_eq!(
        normalize_registry(&method, crate::binding::Language::Rust, "fixture.method").unwrap_err(),
        "operation 'fixture.method::increment' is a method with no receiver setup; annotate a #[spec_setup(\"increment\")] producer for its receiver"
    );

    let dynamic = registry(
        r#"{"operations":[
                {"name":"echo","module_path":"fixture","fn_name":"echo","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"Value","fills":"","params":[["input","i32"]],"component":"fixture.value"}
            ],"types":[]}"#,
    );
    assert_eq!(
        normalize_registry(&dynamic, crate::binding::Language::Rust, "fixture.value").unwrap_err(),
        "operation 'fixture.value::echo' output type 'value' is the dynamic runtime value; CTSC registries require declared semantic types"
    );

    let dynamic_field = registry(
        r#"{"operations":[
                {"name":"snapshot","module_path":"fixture","fn_name":"snapshot","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"Record","fills":"","params":[],"component":"fixture.value"}
            ],"types":[{"name":"Record","module_path":"fixture","kind":"struct","component":"fixture.value","fields":[["history","Vec<Value>"]],"variants":[]}]}"#,
    );
    assert_eq!(
        normalize_registry(&dynamic_field, crate::binding::Language::Rust, "fixture.value").unwrap_err(),
        "type 'Record' field 'history' type 'List<value>' is the dynamic runtime value; CTSC registries require declared semantic types"
    );
}

#[test]
fn csharp_metadata() {
    let registry = registry(
        r#"{"operations":[{
                "name":"add","module_path":"","fn_name":"","is_setup":false,"is_async":true,"is_method":false,"is_public":true,
                "return_type":"i32","fills":"","params":[["a","i32"]],"component":"fixture.add",
                "cs_class":"Fixtures.Math","cs_method_of":"Math","cs_method":"Add","cs_is_static":true,
                "cs_return":"Task<int>","cs_params":[["a","int"]],"cs_exceptions":["InvalidOperationException"]
            }],"types":[]}"#,
    );
    let operation = &registry.ops[0];
    assert_eq!(operation.cs_class.as_deref(), Some("Fixtures.Math"));
    assert_eq!(operation.cs_method.as_deref(), Some("Add"));
    assert_eq!(operation.cs_return.as_deref(), Some("Task<int>"));
    assert_eq!(operation.cs_params[0].name, "a");
    assert_eq!(operation.cs_params[0].ty, "int");
    assert_eq!(
        operation
            .cs_exceptions
            .as_ref()
            .map(|exceptions| exceptions.iter().map(TypeName::as_str).collect::<Vec<_>>()),
        Some(vec!["InvalidOperationException"])
    );
}

#[test]
fn nested_units() {
    assert_eq!(map("Option<()>", []).unwrap().ref_string(), "Option<unit>");
    assert_eq!(map("Result<(), String>", []).unwrap().ref_string(), "Result<unit, string>");

    let registry = registry(
        r#"{"operations":[
            {
                "name":"fallible","module_path":"fixture","fn_name":"fallible","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"Result<(), String>",
                "fills":"","params":[],"component":"fixture.unit"
            },
            {
                "name":"optional","module_path":"fixture","fn_name":"optional","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"Option<()>",
                "fills":"","params":[],"component":"fixture.unit"
            }],"types":[]}"#,
    );
    let schema = normalize_registry(&registry, crate::binding::Language::Rust, "fixture.unit").unwrap();
    let fallible = schema.operations.iter().find(|operation| operation.name == "fallible").unwrap();
    assert!(fallible.output.is_none());
    assert_eq!(fallible.errors[0].ty.as_deref(), Some("string"));
    let optional = schema.operations.iter().find(|operation| operation.name == "optional").unwrap();
    assert_eq!(optional.output.as_deref(), Some("Option<unit>"));
    assert!(!optional.empty);
}

#[test]
fn invalid_tokens() {
    assert!(parse("Option<i32> trailing").is_none());
    assert!(parse("Result<(), String").is_none());
    assert!(parse("[i32").is_none());
}

#[test]
fn qualified_dependencies() {
    let registry = registry(
        r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Shared"]],"component":"fixture.app"
            }],"types":[{
                "name":"Shared","module_path":"fixture","kind":"struct","component":"fixture.shared",
                "fields":[["id","i32"]],"variants":[]
            }]}"#,
    );
    let schema = normalize_registry(&registry, crate::binding::Language::Rust, "fixture.app").unwrap();
    assert_eq!(schema.dependencies, vec!["fixture.shared"]);
    assert_eq!(schema.operations[0].inputs[0].ty, "fixture.shared::Shared");
    assert_eq!(schema.dependency_types[0].component, "fixture.shared");
    assert!(schema.dependency_types[0].dependencies.is_empty());
    assert_eq!(schema.dependency_types[0].types[0].name, "Shared");
}

#[test]
fn transitive_dependencies() {
    let registry = registry(
        r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Middle"]],"component":"fixture.app"
            }],"types":[
                {"name":"Middle","module_path":"fixture","kind":"struct","component":"fixture.middle","fields":[["leaf","Leaf"]],"variants":[]},
                {"name":"Leaf","module_path":"fixture","kind":"struct","component":"fixture.leaf","fields":[["id","i32"]],"variants":[]}
            ]}"#,
    );
    let schema = normalize_registry(&registry, crate::binding::Language::Rust, "fixture.app").unwrap();
    assert_eq!(schema.dependencies, vec!["fixture.middle"]);
    assert_eq!(schema.dependency_types.len(), 2);
    let middle = schema
        .dependency_types
        .iter()
        .find(|dependency| dependency.component == "fixture.middle")
        .unwrap();
    assert_eq!(middle.dependencies, vec!["fixture.leaf"]);
    assert_eq!(middle.types[0].fields[0].ty, "fixture.leaf::Leaf");
}

#[test]
fn invalid_dependencies() {
    let cycle = registry(
        r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Middle"]],"component":"fixture.app"
            }],"types":[
                {"name":"AppType","module_path":"fixture","kind":"struct","component":"fixture.app","fields":[],"variants":[]},
                {"name":"Middle","module_path":"fixture","kind":"struct","component":"fixture.middle","fields":[["app","AppType"]],"variants":[]}
            ]}"#,
    );
    assert!(
        normalize_registry(&cycle, crate::binding::Language::Rust, "fixture.app")
            .unwrap_err()
            .contains("fixture.app -> fixture.middle -> fixture.app")
    );

    let missing = registry(
        r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Missing"]],"component":"fixture.app"
            }],"types":[]}"#,
    );
    assert!(
        normalize_registry(&missing, crate::binding::Language::Rust, "fixture.app")
            .unwrap_err()
            .contains("Missing")
    );
}

#[test]
fn candidate_runtime() {
    let context = crate::runner::CandidatePackage::new(crate::runner::CandidateDeps {
        package: "candidate".into(),
        version: "1.2.3".into(),
        path: PathBuf::from("candidate-package"),
        runtime: crate::runner::PackageSource::local("specgate-runtime", "0.6.0", "resolved-runtime"),
    });
    let cargo = discovery_cargo(&context).unwrap();
    let parsed: toml::Value = toml::from_str(&cargo.manifest).unwrap();
    assert_eq!(parsed["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
    assert_eq!(
        parsed["dependencies"]["specgate_runtime"]["path"].as_str(),
        Some("resolved-runtime")
    );
    assert_eq!(parsed["dependencies"]["candidate"]["package"].as_str(), Some("candidate"));
    assert_eq!(cargo.config, None);
}

#[test]
fn runner_failures() {
    use crate::runner::system::{FailureKey, FakeFs, FakeProcess, ProcessOutput};
    use std::collections::{BTreeMap, VecDeque};

    fn context() -> crate::runner::CandidatePackage {
        crate::runner::CandidatePackage::new(crate::runner::CandidateDeps {
            package: "candidate".into(),
            version: "1.0.0".into(),
            path: PathBuf::from("candidate"),
            runtime: crate::runner::PackageSource::local("specgate-runtime", "0.6.0", "runtime"),
        })
    }
    fn system(result: Result<ProcessOutput, String>, failures: BTreeMap<String, VecDeque<String>>) -> crate::runner::system::System {
        crate::runner::system::System::builder((
            FakeFs::builder()
                .failures(failures.into_iter().map(|(key, errors)| (FailureKey::test(&key), errors)).collect())
                .unique_candidates([Ok(PathBuf::from("scratch/run"))].into())
                .build(),
            FakeProcess::queued([result]),
        ))
        .environment(BTreeMap::from([
            ("SPECGATE_CACHE_DIR".to_string(), "cache".into()),
            ("CARGO".to_string(), "fake-cargo".into()),
        ]))
        .current_directory(Ok(PathBuf::from("cwd")))
        .process_id(7)
        .build()
    }

    let cases = [
        (Err("spawn denied".to_string()), "failed to run discovery build: spawn denied"),
        (
            Ok(ProcessOutput {
                success: false,
                stdout: Vec::new(),
                stderr: b"compile denied\n".to_vec(),
            }),
            "discovery build failed: compile denied",
        ),
        (
            Ok(ProcessOutput {
                success: true,
                stdout: b"  \n".to_vec(),
                stderr: Vec::new(),
            }),
            "discovery build produced no output",
        ),
    ];
    for (result, expected) in cases {
        let system = system(result, BTreeMap::new());
        assert_eq!(runner::run_in(&context(), &system).unwrap_err(), expected);
        let request = system.process.fake_state().lock().unwrap().requests[0].clone();
        assert_eq!(request.executable, std::ffi::OsString::from("fake-cargo"));
        assert!(request.env_remove.contains(&std::ffi::OsString::from("RUSTC_WORKSPACE_WRAPPER")));
    }

    for (key, expected) in [
        ("create_dir_all", "failed to scaffold discovery crate"),
        ("write:Cargo.toml", "failed to write discovery manifest"),
        ("write:main.rs", "failed to write discovery main.rs"),
    ] {
        let failures = BTreeMap::from([(key.to_string(), ["denied".to_string()].into())]);
        let system = system(
            Ok(ProcessOutput {
                success: true,
                stdout: b"{}".to_vec(),
                stderr: Vec::new(),
            }),
            failures,
        );
        assert!(runner::run_in(&context(), &system).unwrap_err().contains(expected));
    }
}

#[test]
fn registry_failure() {
    use crate::runner::system::{FailureKey, FakeFs, FakeProcess, ProcessOutput};
    use std::collections::{BTreeMap, VecDeque};
    let context = crate::runner::CandidatePackage::new(crate::runner::CandidateDeps {
        package: "candidate".into(),
        version: "1.0.0".into(),
        path: PathBuf::from("candidate"),
        runtime: crate::runner::PackageSource::registry("specgate-runtime", "0.6.0", "sparse+https://registry.example.invalid/index/"),
    });
    let system = crate::runner::system::System::builder((
        FakeFs::builder()
            .failures(BTreeMap::from([(
                FailureKey::test("write:registry-config.toml"),
                VecDeque::from(["config denied".to_string()]),
            )]))
            .unique_candidates(VecDeque::from([Ok(PathBuf::from("scratch/run"))]))
            .build(),
        FakeProcess::queued([Ok(ProcessOutput {
            success: true,
            stdout: b"{}".to_vec(),
            stderr: Vec::new(),
        })]),
    ))
    .environment(BTreeMap::from([("SPECGATE_CACHE_DIR".to_string(), "cache".into())]))
    .current_directory(Ok(PathBuf::from("cwd")))
    .process_id(7)
    .build();
    assert_eq!(
        runner::run_in(&context, &system).unwrap_err(),
        "failed to write discovery registry config: config denied"
    );
    let state = system.filesystem.fake_state();
    let state = state.borrow();
    assert!(state.has_directory(Path::new("scratch/run/src")));
    assert!(state.has_file(Path::new("scratch/run/Cargo.toml")));
    assert!(!state.has_file(Path::new("scratch/run/src/main.rs")));
    assert!(system.process.fake_state().lock().unwrap().requests.is_empty());
}
