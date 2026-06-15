use specgate_types::BindingFile;

fn parse_binding(input: &str) -> Result<BindingFile, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

fn expect_valid(input: &str) -> BindingFile {
    parse_binding(input).expect("binding should parse")
}

fn expect_invalid(input: &str, reason: &str) {
    let error = parse_binding(input).expect_err("binding should be invalid");
    assert_eq!(error.to_string(), reason);
}

#[test]
fn valid_command_target() {
    let binding = expect_valid(
        r#"
language: rust
targets:
  test:
    package_root: ../rust/crates/my-app
    test_root: ../rust/crates/my-app/tests
    build: "cargo build"
    command: "cargo test -p my-app --test specgate_generated"
    outputs:
      stdout: json
"#,
    );

    assert_eq!(binding.language, "rust");
    assert_eq!(binding.targets.len(), 1);
    assert_eq!(
        binding.targets["test"].package_root,
        "../rust/crates/my-app"
    );
    assert_eq!(
        binding.targets["test"].test_root.as_deref(),
        Some("../rust/crates/my-app/tests")
    );
    assert!(binding.targets["test"].is_command());
    assert!(!binding.targets["test"].is_api());
}

#[test]
fn valid_api_target() {
    let binding = expect_valid(
        r#"
language: rust
targets:
  generate:
    package_root: ../rust/crates/my-backend
    function: "my_backend::RustBackend::generate"
    constructor: "my_backend::RustBackend::new"
"#,
    );

    assert_eq!(binding.language, "rust");
    assert_eq!(binding.targets.len(), 1);
    assert_eq!(
        binding.targets["generate"].package_root,
        "../rust/crates/my-backend"
    );
    assert_eq!(binding.targets["generate"].test_root, None);
    assert!(binding.targets["generate"].is_api());
    assert!(!binding.targets["generate"].is_command());
}

#[test]
fn multiple_targets() {
    let binding = expect_valid(
        r#"
language: rust
targets:
  test:
    package_root: ../rust/crates/my-app
    command: "cargo test -p my-app"
  generate:
    package_root: ../rust/crates/my-backend
    function: "backend::generate"
"#,
    );

    assert_eq!(binding.language, "rust");
    assert_eq!(binding.targets.len(), 2);
    assert_eq!(
        binding.targets["test"].package_root,
        "../rust/crates/my-app"
    );
    assert_eq!(
        binding.targets["generate"].package_root,
        "../rust/crates/my-backend"
    );
}

#[test]
fn missing_language() {
    expect_invalid(
        r#"
targets: {}
"#,
        "missing required field 'language'",
    );
}

#[test]
fn empty_targets() {
    let binding = expect_valid(
        r#"
language: csharp
"#,
    );

    assert_eq!(binding.language, "csharp");
    assert!(binding.targets.is_empty());
}

#[test]
fn target_with_both_command_and_function() {
    expect_invalid(
        r#"
language: rust
targets:
  test:
    package_root: ../rust/crates/my-app
    command: "cargo test -p my-app"
    function: "backend::run"
"#,
        "target cannot have both command and function",
    );
}

#[test]
fn target_with_neither_command_nor_function() {
    let binding = expect_valid(
        r#"
language: rust
targets:
  test:
    package_root: ../rust/crates/my-app
    build: "cargo build"
"#,
    );
    assert_eq!(binding.language, "rust");
    assert_eq!(binding.targets.len(), 1);
}

#[test]
fn target_with_file_output() {
    let binding = expect_valid(
        r#"
language: rust
targets:
  test:
    package_root: ../rust/crates/my-app
    command: "cargo test -p my-app"
    outputs:
      file: "{workdir}/results.json"
"#,
    );

    assert_eq!(binding.language, "rust");
    assert_eq!(binding.targets.len(), 1);
    assert_eq!(
        binding.targets["test"].package_root,
        "../rust/crates/my-app"
    );
    assert_eq!(
        binding.targets["test"].outputs.file.as_deref(),
        Some("{workdir}/results.json")
    );
}

#[test]
fn target_missing_package_root() {
    expect_invalid(
        r#"
language: rust
targets:
  test:
    command: "cargo test"
"#,
        "missing required field 'package_root'",
    );
}
