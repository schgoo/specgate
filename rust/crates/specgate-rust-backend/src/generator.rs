use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use specgate_annotations::*;
use specgate_types::{BindingTarget, SpecCase, SpecDocument};

use crate::annotations::{Annotation, OperationKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SpecCapture)]
#[spec_capture("harness.rust")]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GenerateError {
    MissingSetup { operation: String },
    MissingCapture { operation: String },
    UnsupportedType { type_name: String, detail: String },
}

#[spec_operation("harness.rust", kind = Sequence)]
pub fn generate_test_file(
    spec: &SpecDocument,
    annotations: &[Annotation],
    binding_target: Option<&BindingTarget>,
    generated_test_path: &Path,
    results_path: &Path,
) -> Result<GeneratedFile, Vec<GenerateError>> {
    let target = resolve_codegen_target(spec, annotations, binding_target)?;
    let content = render_generated_file(spec, &target, results_path)?;

    Ok(GeneratedFile {
        path: generated_test_path.to_path_buf(),
        content,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CodegenTarget {
    Annotation {
        operation: OperationAnnotation,
        setup: SetupAnnotation,
        captures: Vec<CaptureAnnotation>,
        checkpoints: Vec<CheckpointAnnotation>,
        mocks: Vec<MockAnnotation>,
    },
    Api(ApiTarget),
    Command(CommandTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiTarget {
    function: String,
    constructor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandTarget {
    command: String,
    output_file: Option<String>,
}

#[spec_setup("harness.rust", name = "default")]
fn resolve_codegen_target(
    spec: &SpecDocument,
    annotations: &[Annotation],
    binding_target: Option<&BindingTarget>,
) -> Result<CodegenTarget, Vec<GenerateError>> {
    if annotations.is_empty() {
        if let Some(binding_target) = binding_target {
            if binding_target.is_command() {
                return resolve_command_target(binding_target);
            }
            if binding_target.is_api() {
                return resolve_api_target(binding_target);
            }
            return Err(vec![GenerateError::UnsupportedType {
                type_name: "binding_target".to_string(),
                detail: "binding target requires command or function field".to_string(),
            }]);
        }
    }

    resolve_annotation_target(spec, annotations)
}

fn resolve_api_target(binding_target: &BindingTarget) -> Result<CodegenTarget, Vec<GenerateError>> {
    let Some(function) = &binding_target.function else {
        return Err(vec![GenerateError::UnsupportedType {
            type_name: "binding_target".to_string(),
            detail: "api target requires function field".to_string(),
        }]);
    };

    Ok(CodegenTarget::Api(ApiTarget {
        function: function.clone(),
        constructor: binding_target.constructor.clone(),
    }))
}

fn resolve_command_target(
    binding_target: &BindingTarget,
) -> Result<CodegenTarget, Vec<GenerateError>> {
    let Some(command) = &binding_target.command else {
        return Err(vec![GenerateError::UnsupportedType {
            type_name: "binding_target".to_string(),
            detail: "command target requires command field".to_string(),
        }]);
    };

    Ok(CodegenTarget::Command(CommandTarget {
        command: command.clone(),
        output_file: binding_target.outputs.file.clone(),
    }))
}

fn resolve_annotation_target(
    spec: &SpecDocument,
    annotations: &[Annotation],
) -> Result<CodegenTarget, Vec<GenerateError>> {
    let operation = find_operation_annotation(spec, annotations).ok_or_else(|| {
        vec![GenerateError::UnsupportedType {
            type_name: "Annotation".to_string(),
            detail: format!("missing SpecOperation annotation for {}", spec.name),
        }]
    })?;

    let setup = find_setup_annotation(spec, annotations);
    let captures = capture_annotations(spec, annotations);
    let checkpoints = checkpoint_annotations(spec, annotations);
    let mocks = mock_annotations(spec, annotations);

    let mut errors = Vec::new();
    if setup.is_none() {
        errors.push(GenerateError::MissingSetup {
            operation: spec.name.clone(),
        });
    }

    if operation.kind == OperationKind::StateMachine && captures.is_empty() {
        errors.push(GenerateError::MissingCapture {
            operation: spec.name.clone(),
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(CodegenTarget::Annotation {
        operation,
        setup: setup.expect("setup presence checked above"),
        captures,
        checkpoints,
        mocks,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperationAnnotation {
    kind: OperationKind,
    symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupAnnotation {
    name: String,
    symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureAnnotation {
    symbol: String,
    capture_all: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CheckpointAnnotation {
    symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MockAnnotation {
    name: String,
    symbol: String,
}

fn find_operation_annotation(
    spec: &SpecDocument,
    annotations: &[Annotation],
) -> Option<OperationAnnotation> {
    annotations.iter().find_map(|annotation| match annotation {
        Annotation::SpecOperation {
            operation,
            kind,
            symbol,
        } if operation == &spec.name => Some(OperationAnnotation {
            kind: kind.clone(),
            symbol: symbol.clone(),
        }),
        _ => None,
    })
}

fn find_setup_annotation(
    spec: &SpecDocument,
    annotations: &[Annotation],
) -> Option<SetupAnnotation> {
    annotations
        .iter()
        .find_map(|annotation| match annotation {
            Annotation::SpecSetup {
                operation,
                name,
                symbol,
                ..
            } if operation == &spec.name && name == "default" => Some(SetupAnnotation {
                name: name.clone(),
                symbol: symbol.clone(),
            }),
            _ => None,
        })
        .or_else(|| {
            annotations.iter().find_map(|annotation| match annotation {
                Annotation::SpecSetup {
                    operation,
                    name,
                    symbol,
                    ..
                } if operation == &spec.name => Some(SetupAnnotation {
                    name: name.clone(),
                    symbol: symbol.clone(),
                }),
                _ => None,
            })
        })
}

fn capture_annotations(spec: &SpecDocument, annotations: &[Annotation]) -> Vec<CaptureAnnotation> {
    annotations
        .iter()
        .filter_map(|annotation| match annotation {
            Annotation::SpecCapture {
                operation,
                symbol,
                capture_all,
            } if operation == &spec.name => Some(CaptureAnnotation {
                symbol: symbol.clone(),
                capture_all: *capture_all,
            }),
            _ => None,
        })
        .collect()
}

fn checkpoint_annotations(
    spec: &SpecDocument,
    annotations: &[Annotation],
) -> Vec<CheckpointAnnotation> {
    annotations
        .iter()
        .filter_map(|annotation| match annotation {
            Annotation::SpecCheckpoint { operation, symbol } if operation == &spec.name => {
                Some(CheckpointAnnotation {
                    symbol: symbol.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

fn mock_annotations(spec: &SpecDocument, annotations: &[Annotation]) -> Vec<MockAnnotation> {
    annotations
        .iter()
        .filter_map(|annotation| match annotation {
            Annotation::SpecMock {
                operation,
                name,
                symbol,
            } if operation == &spec.name => Some(MockAnnotation {
                name: name.clone(),
                symbol: symbol.clone(),
            }),
            _ => None,
        })
        .collect()
}

#[spec_checkpoint("harness.rust")]
fn render_generated_file(
    spec: &SpecDocument,
    target: &CodegenTarget,
    results_path: &Path,
) -> Result<String, Vec<GenerateError>> {
    let mut file = String::new();
    file.push_str("#![allow(unused_variables)]\n");
    file.push_str("#![allow(clippy::all)]\n\n");
    file.push_str("use std::fs::{self, OpenOptions};\n");
    file.push_str("use std::io::Write;\n");
    file.push_str("use std::path::PathBuf;\n");
    file.push_str("use std::process::Command;\n");
    file.push_str("use std::time::Instant;\n");
    file.push_str("use serde_json::json;\n\n");
    file.push_str("fn specgate_results_path() -> PathBuf {\n");
    file.push_str("    std::env::var_os(\"SPECGATE_RESULTS_PATH\")\n");
    file.push_str("        .map(PathBuf::from)\n");
    file.push_str(&format!(
        "        .unwrap_or_else(|| PathBuf::from({}))\n",
        string_literal(&results_path.display().to_string())
    ));
    file.push_str("}\n\n");
    file.push_str("fn specgate_traces_path(name: &str) -> PathBuf {\n");
    file.push_str(
        "    PathBuf::from(\"target\").join(\"specgate-harness\").join(\"traces\").join(format!(\"{name}.json\"))\n",
    );
    file.push_str("}\n\n");
    file.push_str(
        "fn specgate_write_traces(name: &str) -> (Vec<specgate::TraceEvent>, Option<String>) {\n",
    );
    file.push_str("    let traces = specgate::runtime::drain_traces();\n");
    file.push_str("    if traces.is_empty() {\n");
    file.push_str("        return (traces, None);\n");
    file.push_str("    }\n");
    file.push_str("    let traces_path = specgate_traces_path(name);\n");
    file.push_str(
        "    if let Some(parent) = traces_path.parent() {\n        fs::create_dir_all(parent).expect(\"specgate traces directory should be creatable\");\n    }\n",
    );
    file.push_str(
        "    fs::write(\n        &traces_path,\n        serde_json::to_string_pretty(&traces).expect(\"specgate traces should serialize\"),\n    )\n    .expect(\"specgate traces file should be writable\");\n",
    );
    file.push_str("    (traces, Some(traces_path.display().to_string().replace('\\\\', \"/\")))\n");
    file.push_str("}\n\n");
    file.push_str(
        "fn specgate_write_result(name: &str, status: &str, duration_ms: i64, traces_file: Option<String>, traces_match: Option<bool>) {\n",
    );
    file.push_str("    let mut file = OpenOptions::new()\n");
    file.push_str("        .create(true)\n");
    file.push_str("        .append(true)\n");
    file.push_str("        .open(specgate_results_path())\n");
    file.push_str("        .expect(\"specgate results file should be writable\");\n");
    file.push_str("    let mut result = json!({\n");
    file.push_str("        \"name\": name,\n");
    file.push_str("        \"status\": status,\n");
    file.push_str("        \"duration_ms\": duration_ms,\n");
    file.push_str("    });\n");
    file.push_str(
        "    if let Some(traces_file) = traces_file {\n        result[\"traces_file\"] = serde_json::Value::String(traces_file);\n    }\n",
    );
    file.push_str(
        "    if let Some(traces_match) = traces_match {\n        result[\"traces_match\"] = serde_json::Value::Bool(traces_match);\n    }\n",
    );
    file.push_str(
        "    writeln!(file, \"{}\", serde_json::to_string(&result).expect(\"specgate result line should serialize\"))\n",
    );
    file.push_str("        .expect(\"specgate result line should be written\");\n");
    file.push_str("}\n\n");
    file.push_str("fn specgate_drain_checkpoints() -> Vec<String> {\n");
    file.push_str("    specgate::runtime::drain_checkpoints()\n");
    file.push_str("}\n\n");
    file.push_str(
        "fn specgate_apply_template(template: &str, replacements: &[(&str, String)]) -> String {\n",
    );
    file.push_str("    let mut rendered = template.to_string();\n");
    file.push_str("    for (key, value) in replacements {\n");
    file.push_str("        rendered = rendered.replace(&format!(\"{{{key}}}\"), value);\n");
    file.push_str("    }\n");
    file.push_str("    rendered\n");
    file.push_str("}\n\n");
    file.push_str("fn specgate_spawn_shell_command(command_line: &str) -> Command {\n");
    file.push_str("    #[cfg(windows)]\n");
    file.push_str("    {\n");
    file.push_str("        let mut command = Command::new(\"cmd\");\n");
    file.push_str("        command.arg(\"/C\").arg(command_line);\n");
    file.push_str("        command\n");
    file.push_str("    }\n");
    file.push_str("    #[cfg(not(windows))]\n");
    file.push_str("    {\n");
    file.push_str("        let mut command = Command::new(\"sh\");\n");
    file.push_str("        command.arg(\"-c\").arg(command_line);\n");
    file.push_str("        command\n");
    file.push_str("    }\n");
    file.push_str("}\n\n");

    let mut render_errors = Vec::new();
    for case in &spec.cases {
        let rendered = match target {
            CodegenTarget::Annotation {
                operation,
                setup,
                captures,
                checkpoints,
                mocks,
            } => render_annotation_case(case, operation, setup, captures, checkpoints, mocks),
            CodegenTarget::Api(target) => render_api_case(case, target),
            CodegenTarget::Command(target) => render_command_case(case, target),
        };

        match rendered {
            Ok(test_body) => {
                file.push_str(&test_body);
                file.push('\n');
            }
            Err(error) => render_errors.push(error),
        }
    }

    if render_errors.is_empty() {
        Ok(file)
    } else {
        Err(render_errors)
    }
}

fn render_annotation_case(
    case: &SpecCase,
    operation: &OperationAnnotation,
    setup: &SetupAnnotation,
    captures: &[CaptureAnnotation],
    checkpoints: &[CheckpointAnnotation],
    mocks: &[MockAnnotation],
) -> Result<String, GenerateError> {
    let input_arguments = render_case_arguments(case)?;
    let capture_source = capture_source(&operation.kind);
    let checkpoint_symbols = checkpoints
        .iter()
        .map(|checkpoint| checkpoint.symbol.clone())
        .collect::<Vec<_>>();

    let mut body = String::new();
    body.push_str("#[test]\n");
    body.push_str(&format!("fn {}() {{\n", sanitize_test_name(&case.name)));
    body.push_str("    specgate::runtime::reset();\n");
    body.push_str("    let specgate_started = Instant::now();\n");
    body.push_str(&format!(
        "    // setup {} via {}\n",
        setup.name, setup.symbol
    ));

    if is_method_symbol(&operation.symbol) {
        body.push_str(&format!(
            "    let mut subject = {}({});\n",
            setup.symbol, input_arguments
        ));
        render_state_machine_before_block(&mut body, &operation.kind, captures);
        body.push_str(&format!(
            "    // operation {}\n    let actual = {}(&mut subject{}{});\n",
            operation.symbol,
            operation.symbol,
            if input_arguments.is_empty() { "" } else { ", " },
            input_arguments
        ));
    } else {
        body.push_str(&format!(
            "    let _setup = {}({});\n",
            setup.symbol, input_arguments
        ));
        body.push_str(&format!(
            "    // operation {}\n    let actual = {}({});\n",
            operation.symbol, operation.symbol, input_arguments
        ));
    }

    render_mock_block(&mut body, case, mocks)?;
    render_capture_block(&mut body, case, &operation.kind, captures, capture_source)?;
    render_checkpoint_block(&mut body, case, &checkpoint_symbols)?;
    body.push_str(&format!(
        "    let (specgate_traces, specgate_traces_file) = specgate_write_traces({});\n",
        string_literal(&case.name)
    ));
    render_traces_match_block(
        &mut body,
        case,
        "serde_json::to_value(&specgate_traces)\n        .expect(\"specgate traces should serialize\")",
    )?;
    body.push_str("    specgate_write_result(\n");
    body.push_str(&format!("        {},\n", string_literal(&case.name)));
    body.push_str("        specgate_status,\n");
    body.push_str("        specgate_started.elapsed().as_millis() as i64,\n");
    body.push_str("        specgate_traces_file,\n");
    body.push_str("        specgate_traces_match,\n");
    body.push_str("    );\n");
    body.push_str("}\n");
    Ok(body)
}

fn render_api_case(case: &SpecCase, target: &ApiTarget) -> Result<String, GenerateError> {
    let input_arguments = render_case_arguments(case)?;
    let mut body = String::new();
    body.push_str("#[test]\n");
    body.push_str(&format!("fn {}() {{\n", sanitize_test_name(&case.name)));
    body.push_str("    specgate::runtime::reset();\n");
    body.push_str("    let specgate_started = Instant::now();\n");

    if let Some(constructor) = &target.constructor {
        body.push_str(
            "    let repo_root = PathBuf::from(env!(\"CARGO_MANIFEST_DIR\"))\n        .parent()\n        .expect(\"manifest dir should have a parent\")\n        .to_path_buf();\n",
        );
        body.push_str(&format!(
            "    let mut subject = {constructor}(repo_root);\n"
        ));
        body.push_str(&format!(
            "    let actual = subject.{}({});\n",
            symbol_tail(&target.function),
            input_arguments
        ));
    } else {
        body.push_str(&format!(
            "    let actual = {}({});\n",
            target.function, input_arguments
        ));
    }

    body.push_str(
        "    let actual_json: serde_json::Value = serde_json::to_value(&actual)\n        .expect(\"api target output should serialize to JSON\");\n",
    );
    render_json_expectations(&mut body, "actual_json", &case.expected)?;
    body.push_str(&format!(
        "    let (specgate_traces, specgate_traces_file) = specgate_write_traces({});\n",
        string_literal(&case.name)
    ));
    render_traces_match_block(&mut body, case, "actual_json[\"traces\"].clone()")?;
    body.push_str("    specgate_write_result(\n");
    body.push_str(&format!("        {},\n", string_literal(&case.name)));
    body.push_str("        specgate_status,\n");
    body.push_str("        specgate_started.elapsed().as_millis() as i64,\n");
    body.push_str("        specgate_traces_file,\n");
    body.push_str("        specgate_traces_match,\n");
    body.push_str("    );\n");
    body.push_str("}\n");
    Ok(body)
}

fn render_command_case(case: &SpecCase, target: &CommandTarget) -> Result<String, GenerateError> {
    let mut body = String::new();
    body.push_str("#[test]\n");
    body.push_str(&format!("fn {}() {{\n", sanitize_test_name(&case.name)));
    body.push_str("    specgate::runtime::reset();\n");
    body.push_str("    let specgate_started = Instant::now();\n");
    body.push_str(
        "    let workdir = specgate_results_path()\n        .parent()\n        .expect(\"results path should have a parent\")\n        .to_path_buf();\n",
    );
    body.push_str(&format!(
        "    let case_dir = workdir.join({});\n",
        string_literal(&case.name)
    ));
    body.push_str(
        "    fs::create_dir_all(&case_dir).expect(\"case workdir should be created\");\n",
    );
    body.push_str(&format!(
        "    let mut replacements: Vec<(&str, String)> = vec![(\"workdir\", workdir.display().to_string()), (\"case_name\", {}.to_string())];\n",
        string_literal(&case.name)
    ));
    for (name, value) in &case.inputs {
        if name.starts_with("mock_") {
            continue;
        }
        match value {
            Value::String(string) => {
                let extension = if name == "source" || name == "driver" {
                    "rs"
                } else {
                    "txt"
                };
                body.push_str(&format!(
                    "    let {name}_path = case_dir.join({});\n",
                    string_literal(&format!("{name}.{extension}"))
                ));
                body.push_str(&format!(
                    "    fs::write(&{name}_path, {}).expect(\"case input should be written\");\n",
                    string_literal(string)
                ));
                body.push_str(&format!(
                    "    replacements.push(({:?}, {name}_path.display().to_string()));\n",
                    name
                ));
            }
            _ => {
                body.push_str(&format!(
                    "    replacements.push(({:?}, {}.to_string()));\n",
                    name,
                    scalar_template_value(value)?
                ));
            }
        }
    }
    body.push_str(&format!(
        "    let command_line = specgate_apply_template({}, &replacements);\n",
        string_literal(&target.command)
    ));
    body.push_str(
        "    let output = specgate_spawn_shell_command(&command_line)\n        .output()\n        .expect(\"command target should run\");\n",
    );
    body.push_str(
        "    assert!(output.status.success(), \"command target failed: {}\", output.status);\n",
    );

    if let Some(output_template) = &target.output_file {
        body.push_str(&format!(
            "    let output_path = specgate_apply_template({}, &replacements);\n",
            string_literal(&output_template)
        ));
        body.push_str(
            "    let stdout = fs::read_to_string(&output_path)\n        .expect(\"command target output file should be readable\");\n",
        );
    } else {
        body.push_str(
            "    let stdout = String::from_utf8(output.stdout)\n        .expect(\"command target stdout should be valid UTF-8\");\n",
        );
    }

    body.push_str(
        "    let actual_json: serde_json::Value = serde_json::from_str(&stdout)\n        .expect(\"command target output should be valid JSON\");\n",
    );
    render_json_expectations(&mut body, "actual_json", &case.expected)?;
    body.push_str(&format!(
        "    let (specgate_traces, specgate_traces_file) = specgate_write_traces({});\n",
        string_literal(&case.name)
    ));
    render_traces_match_block(&mut body, case, "actual_json[\"traces\"].clone()")?;
    body.push_str("    specgate_write_result(\n");
    body.push_str(&format!("        {},\n", string_literal(&case.name)));
    body.push_str("        specgate_status,\n");
    body.push_str("        specgate_started.elapsed().as_millis() as i64,\n");
    body.push_str("        specgate_traces_file,\n");
    body.push_str("        specgate_traces_match,\n");
    body.push_str("    );\n");
    body.push_str("}\n");
    Ok(body)
}

fn render_mock_block(
    body: &mut String,
    case: &SpecCase,
    mocks: &[MockAnnotation],
) -> Result<(), GenerateError> {
    for mock in mocks {
        let input_key = format!("mock_{}", mock.name);
        let mock_value = case.inputs.get(&input_key).unwrap_or(&Value::Null);
        body.push_str(&format!("    // mock {} via {}\n", mock.name, mock.symbol));
        body.push_str(&format!(
            "    let {} = {};\n",
            input_key,
            render_value(mock_value)?
        ));
        body.push_str(&format!(
            "    specgate::runtime::install_mock({}, &{});\n",
            string_literal(&mock.name),
            input_key
        ));
    }

    Ok(())
}

fn render_state_machine_before_block(
    body: &mut String,
    kind: &OperationKind,
    captures: &[CaptureAnnotation],
) {
    if kind != &OperationKind::StateMachine {
        return;
    }

    for capture in captures {
        if capture.capture_all {
            continue;
        }

        let field_name = symbol_tail(&capture.symbol);
        body.push_str(&format!(
            "    let before_{field_name} = subject.{field_name}.clone();\n"
        ));
    }
}

fn render_capture_block(
    body: &mut String,
    case: &SpecCase,
    kind: &OperationKind,
    captures: &[CaptureAnnotation],
    capture_source: &str,
) -> Result<(), GenerateError> {
    let expected = expected_outputs(case);
    for capture in captures {
        if capture.capture_all {
            for field_name in expected.keys().filter(|key| *key != "checkpoints") {
                body.push_str(&format!(
                    "    let actual_{field_name} = {capture_source}.{field_name}.clone();\n"
                ));
                let value = expected
                    .get(field_name)
                    .expect("field names came from expected.keys()");
                body.push_str(&format!(
                    "    assert_eq!(actual_{field_name}, {});\n",
                    render_value(value)?
                ));
            }
            continue;
        }

        let field_name = symbol_tail(&capture.symbol);
        if kind == &OperationKind::StateMachine {
            body.push_str(&format!(
                "    let after_{field_name} = {capture_source}.{field_name}.clone();\n"
            ));
        } else {
            body.push_str(&format!(
                "    let actual_{field_name} = {capture_source}.{field_name}.clone();\n"
            ));
        }

        if let Some(value) = expected.get(&field_name) {
            let rendered_value = render_value(value)?;
            if kind == &OperationKind::StateMachine {
                body.push_str(&format!(
                    "    assert_eq!(after_{field_name}, {rendered_value});\n"
                ));
            } else {
                body.push_str(&format!(
                    "    assert_eq!(actual_{field_name}, {rendered_value});\n"
                ));
            }
        }
    }

    Ok(())
}

fn render_checkpoint_block(
    body: &mut String,
    case: &SpecCase,
    checkpoint_symbols: &[String],
) -> Result<(), GenerateError> {
    let Some(expected_checkpoints) = case.expected.get("checkpoints") else {
        return Ok(());
    };

    body.push_str(&format!(
        "    // checkpoints: {}\n",
        checkpoint_symbols.join(", ")
    ));
    body.push_str("    let checkpoints = specgate_drain_checkpoints();\n");
    body.push_str(&format!(
        "    assert_eq!(checkpoints, {});\n",
        render_string_vec(expected_checkpoints)?
    ));
    Ok(())
}

fn render_traces_match_block(
    body: &mut String,
    case: &SpecCase,
    actual_traces_expr: &str,
) -> Result<(), GenerateError> {
    if let Some(expected_traces) = case.expected.get("traces") {
        body.push_str(&format!(
            "    let specgate_traces_match = Some({actual_traces_expr} == {});\n",
            render_json_value(expected_traces)?
        ));
        body.push_str(
            "    let specgate_status = if specgate_traces_match == Some(false) { \"fail\" } else { \"pass\" };\n",
        );
    } else {
        body.push_str("    let specgate_traces_match = None;\n");
        body.push_str("    let specgate_status = \"pass\";\n");
    }

    Ok(())
}

fn render_case_arguments(case: &SpecCase) -> Result<String, GenerateError> {
    let mut arguments = Vec::new();
    for (name, value) in &case.inputs {
        if name.starts_with("mock_") {
            continue;
        }
        arguments.push(render_value(value)?);
    }
    Ok(arguments.join(", "))
}

fn expected_outputs(case: &SpecCase) -> BTreeMap<String, &Value> {
    case.expected
        .iter()
        .filter(|(name, _)| name.as_str() != "outcome")
        .map(|(name, value)| (name.clone(), value))
        .collect()
}

fn render_json_expectations(
    body: &mut String,
    actual_json: &str,
    expected: &BTreeMap<String, Value>,
) -> Result<(), GenerateError> {
    for (field_name, value) in expected {
        if field_name == "annotations" {
            body.push_str(&format!(
                "    let mut actual_annotations = {}[\"annotations\"].as_array().expect(\"annotations should be an array\").clone();\n",
                actual_json
            ));
            body.push_str(&format!(
                "    let mut expected_annotations = {}.as_array().expect(\"expected annotations should be an array\").clone();\n",
                render_json_value(value)?
            ));
            body.push_str("    actual_annotations.sort_by_key(|value| value.to_string());\n");
            body.push_str("    expected_annotations.sort_by_key(|value| value.to_string());\n");
            body.push_str("    assert_eq!(actual_annotations, expected_annotations);\n");
            continue;
        }
        render_json_assertion(body, actual_json, &[field_name.as_str()], value)?;
    }

    Ok(())
}

fn render_json_assertion(
    body: &mut String,
    actual_json: &str,
    path: &[&str],
    value: &Value,
) -> Result<(), GenerateError> {
    if let Value::Mapping(mapping) = value {
        if mapping.is_empty() {
            body.push_str(&format!(
                "    assert_eq!({}, {});\n",
                render_json_path(actual_json, path),
                render_json_value(value)?
            ));
            return Ok(());
        }

        for (key, child_value) in mapping {
            let Value::String(key) = key else {
                return Err(GenerateError::UnsupportedType {
                    type_name: format!("{key:?}"),
                    detail: "mapping keys must be strings".to_string(),
                });
            };

            let mut child_path = path.to_vec();
            child_path.push(key.as_str());
            render_json_assertion(body, actual_json, &child_path, child_value)?;
        }
        return Ok(());
    }

    body.push_str(&format!(
        "    assert_eq!({}, {});\n",
        render_json_path(actual_json, path),
        render_json_value(value)?
    ));
    Ok(())
}

fn render_json_path(actual_json: &str, path: &[&str]) -> String {
    let mut rendered = actual_json.to_string();
    for segment in path {
        rendered.push('[');
        rendered.push_str(&string_literal(segment));
        rendered.push(']');
    }
    rendered
}

fn render_json_value(value: &Value) -> Result<String, GenerateError> {
    Ok(format!("json!({})", render_json_literal(value)?))
}

fn render_json_literal(value: &Value) -> Result<String, GenerateError> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Number(number) => Ok(number.to_string()),
        Value::String(string) => Ok(string_literal(string)),
        Value::Sequence(values) => {
            let rendered = values
                .iter()
                .map(render_json_literal)
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            Ok(format!("[{rendered}]"))
        }
        Value::Mapping(mapping) => {
            let mut entries = Vec::new();
            for (key, value) in mapping {
                let Value::String(key) = key else {
                    return Err(GenerateError::UnsupportedType {
                        type_name: format!("{key:?}"),
                        detail: "mapping keys must be strings".to_string(),
                    });
                };
                entries.push(format!(
                    "{}: {}",
                    string_literal(key),
                    render_json_literal(value)?
                ));
            }
            Ok(format!("{{{}}}", entries.join(", ")))
        }
        Value::Tagged(tagged) => Err(GenerateError::UnsupportedType {
            type_name: format!("{:?}", tagged.tag),
            detail: "tagged YAML values are not supported".to_string(),
        }),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[spec_mock("harness.rust", name = "case-template")]
fn apply_case_template(template: &str, case: &SpecCase) -> Result<String, GenerateError> {
    let mut rendered = template.to_string();
    for (name, value) in &case.inputs {
        if name.starts_with("mock_") {
            continue;
        }

        rendered = rendered.replace(&format!("{{{name}}}"), &scalar_template_value(value)?);
    }

    Ok(rendered)
}

fn scalar_template_value(value: &Value) -> Result<String, GenerateError> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Number(number) => Ok(number.to_string()),
        Value::String(string) => Ok(string.clone()),
        Value::Sequence(_) | Value::Mapping(_) => Err(GenerateError::UnsupportedType {
            type_name: "binding_target".to_string(),
            detail: "command template values must be scalar".to_string(),
        }),
        Value::Tagged(tagged) => Err(GenerateError::UnsupportedType {
            type_name: format!("{:?}", tagged.tag),
            detail: "tagged YAML values are not supported".to_string(),
        }),
    }
}

fn capture_source(kind: &OperationKind) -> &'static str {
    match kind {
        OperationKind::StateMachine => "subject",
        OperationKind::Stateless
        | OperationKind::Sequence
        | OperationKind::ErrorMap
        | OperationKind::Structural => "actual",
    }
}

fn render_string_vec(value: &Value) -> Result<String, GenerateError> {
    let Value::Sequence(values) = value else {
        return Err(GenerateError::UnsupportedType {
            type_name: "Value".to_string(),
            detail: "expected a sequence for checkpoint assertions".to_string(),
        });
    };

    let rendered = values
        .iter()
        .map(render_value)
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!("vec![{rendered}]"))
}

fn render_value(value: &Value) -> Result<String, GenerateError> {
    match value {
        Value::Null => Ok("serde_json::Value::Null".to_string()),
        Value::Bool(boolean) => Ok(boolean.to_string()),
        Value::Number(number) => Ok(number.to_string()),
        Value::String(string) => Ok(string_literal(string)),
        Value::Sequence(values) => {
            let rendered = values
                .iter()
                .map(render_value)
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            Ok(format!("vec![{rendered}]"))
        }
        Value::Mapping(mapping) => render_mapping(mapping),
        Value::Tagged(tagged) => Err(GenerateError::UnsupportedType {
            type_name: format!("{:?}", tagged.tag),
            detail: "tagged YAML values are not supported".to_string(),
        }),
    }
}

fn render_mapping(mapping: &Mapping) -> Result<String, GenerateError> {
    let mut entries = Vec::new();
    for (key, value) in mapping {
        let Value::String(key) = key else {
            return Err(GenerateError::UnsupportedType {
                type_name: format!("{key:?}"),
                detail: "mapping keys must be strings".to_string(),
            });
        };
        entries.push(format!("{}: {}", string_literal(key), render_value(value)?));
    }
    Ok(format!("json!({{{}}})", entries.join(", ")))
}

fn sanitize_test_name(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
    }
    sanitized
}

fn is_method_symbol(symbol: &str) -> bool {
    symbol.split("::").count() >= 3
}

fn symbol_tail(symbol: &str) -> String {
    symbol.split("::").last().unwrap_or(symbol).to_string()
}

fn string_literal(value: &str) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::{
        GenerateError, apply_case_template, generate_test_file, render_json_expectations,
        render_json_value, render_string_vec, render_value, sanitize_test_name,
        scalar_template_value,
    };
    use crate::annotations::{Annotation, OperationKind};
    use serde_yaml::{Mapping, Value};
    use specgate_types::{
        BindingDecl, BindingEntry, BindingTarget, BindingTargetOutputs, SpecCase, SpecDocument,
    };
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn stateless_operation() {
        let file = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "basic_add",
                    inputs([("a", number(2)), ("b", number(3))]),
                    expected([("outcome", string("Ok")), ("result", number(5))]),
                )],
            ),
            &[
                operation("other", OperationKind::Stateless, "other::noop"),
                operation("calc", OperationKind::Stateless, "calc::Calculator::add"),
                setup("calc", "default", "calc::setup_calc"),
                capture("calc", "calc::Calculator::result"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert_eq!(file.path, Path::new("tests").join("specgate_generated.rs"));
        assert!(file.content.contains("calc::setup_calc(2, 3)"));
        assert!(
            file.content
                .contains("calc::Calculator::add(&mut subject, 2, 3)")
        );
        assert!(
            file.content
                .contains("let actual_result = actual.result.clone();")
        );
        assert!(file.content.contains("assert_eq!(actual_result, 5);"));
    }

    #[test]
    fn statemachine_operation() {
        let file = generate_test_file(
            &spec_with_cases(
                "breaker",
                vec![spec_case(
                    "trip_on_failure",
                    inputs([("success", boolean(false))]),
                    expected([
                        ("outcome", string("Ok")),
                        ("state", string("open")),
                        ("failure_count", number(1)),
                    ]),
                )],
            ),
            &[
                operation(
                    "breaker",
                    OperationKind::StateMachine,
                    "cb::CircuitBreaker::on_result",
                ),
                setup("breaker", "default", "cb::setup_breaker"),
                capture("breaker", "cb::CircuitBreaker::state"),
                capture("breaker", "cb::CircuitBreaker::failure_count"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let before_state = subject.state.clone();")
        );
        assert!(
            file.content
                .contains("let after_state = subject.state.clone();")
        );
        assert!(
            file.content
                .contains("let before_failure_count = subject.failure_count.clone();")
        );
        assert!(
            file.content
                .contains("let after_failure_count = subject.failure_count.clone();")
        );
        assert!(file.content.contains("assert_eq!(after_state, \"open\");"));
        assert!(file.content.contains("assert_eq!(after_failure_count, 1);"));
    }

    #[test]
    fn sequence_with_checkpoints() {
        let file = generate_test_file(
            &spec_with_cases(
                "pipeline",
                vec![spec_case(
                    "two_step",
                    inputs([("input", string("hello"))]),
                    expected([
                        ("outcome", string("Ok")),
                        (
                            "checkpoints",
                            Value::Sequence(vec![string("validated"), string("parsed")]),
                        ),
                    ]),
                )],
            ),
            &[
                operation(
                    "pipeline",
                    OperationKind::Sequence,
                    "pipe::Pipeline::process",
                ),
                setup("pipeline", "default", "pipe::setup_pipeline"),
                checkpoint("pipeline", "pipe::Pipeline::validate"),
                checkpoint("pipeline", "pipe::Pipeline::parse"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains("specgate_drain_checkpoints()"));
        assert!(
            file.content
                .contains("// checkpoints: pipe::Pipeline::validate, pipe::Pipeline::parse")
        );
        assert!(
            file.content
                .contains("assert_eq!(checkpoints, vec![\"validated\", \"parsed\"]);")
        );
    }

    #[test]
    fn mock_injection() {
        let file = generate_test_file(
            &spec_with_cases(
                "fetch",
                vec![spec_case(
                    "success_response",
                    inputs([
                        ("url", string("https://example.com")),
                        (
                            "mock_backend",
                            json_mapping([("status", number(200)), ("body", string("ok"))]),
                        ),
                    ]),
                    expected([("outcome", string("Ok")), ("status", number(200))]),
                )],
            ),
            &[
                operation("fetch", OperationKind::Stateless, "http::Fetcher::fetch"),
                setup("fetch", "default", "http::setup_fetcher"),
                mock_annotation("fetch", "backend", "http::Fetcher::call_backend"),
                capture("fetch", "http::FetchResult::status"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let mock_backend = json!({\"status\": 200, \"body\": \"ok\"});")
        );
        assert!(
            file.content
                .contains("install_mock(\"backend\", &mock_backend)")
        );
        assert!(
            file.content
                .contains("// mock backend via http::Fetcher::call_backend")
        );
    }

    #[test]
    fn multiple_cases_one_file() {
        let file = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![
                    spec_case(
                        "add_positive",
                        inputs([("a", number(2)), ("b", number(3))]),
                        expected([("outcome", string("Ok")), ("result", number(5))]),
                    ),
                    spec_case(
                        "add_negative",
                        inputs([("a", number(-1)), ("b", number(1))]),
                        expected([("outcome", string("Ok")), ("result", number(0))]),
                    ),
                    spec_case(
                        "add_zero",
                        inputs([("a", number(0)), ("b", number(0))]),
                        expected([("outcome", string("Ok")), ("result", number(0))]),
                    ),
                ],
            ),
            &[
                operation("calc", OperationKind::Stateless, "calc::Calculator::add"),
                setup("calc", "default", "calc::setup_calc"),
                capture("calc", "calc::Calculator::result"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains("fn add_positive()"));
        assert!(file.content.contains("fn add_negative()"));
        assert!(file.content.contains("fn add_zero()"));
    }

    #[test]
    fn struct_level_capture() {
        let file = generate_test_file(
            &spec_with_cases(
                "fetch",
                vec![spec_case(
                    "basic_fetch",
                    inputs([("url", string("https://example.com"))]),
                    expected([
                        ("outcome", string("Ok")),
                        ("status_code", number(200)),
                        ("body", string("ok")),
                    ]),
                )],
            ),
            &[
                operation("fetch", OperationKind::Stateless, "http::fetch"),
                setup("fetch", "default", "http::setup_fetch"),
                capture_all("fetch", "http::FetchResult"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let actual_status_code = actual.status_code.clone();")
        );
        assert!(
            file.content
                .contains("let actual_body = actual.body.clone();")
        );
        assert!(
            file.content
                .contains("assert_eq!(actual_status_code, 200);")
        );
        assert!(file.content.contains("assert_eq!(actual_body, \"ok\");"));
    }

    #[test]
    fn missing_setup() {
        let error = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "basic",
                    inputs([("a", number(1))]),
                    expected([("outcome", string("Ok"))]),
                )],
            ),
            &[operation("calc", OperationKind::Stateless, "calc::add")],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert_eq!(
            error,
            vec![GenerateError::MissingSetup {
                operation: "calc".to_string(),
            }]
        );
    }

    #[test]
    fn missing_capture() {
        let error = generate_test_file(
            &spec_with_cases(
                "breaker",
                vec![spec_case(
                    "trip",
                    inputs([("success", boolean(false))]),
                    expected([("outcome", string("Ok"))]),
                )],
            ),
            &[
                operation("breaker", OperationKind::StateMachine, "cb::on_result"),
                setup("breaker", "default", "cb::setup"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert_eq!(
            error,
            vec![GenerateError::MissingCapture {
                operation: "breaker".to_string(),
            }]
        );
    }

    #[test]
    fn missing_operation_annotation() {
        let error = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "basic",
                    inputs([("a", number(1))]),
                    expected([("outcome", string("Ok"))]),
                )],
            ),
            &[],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert!(matches!(
            &error[0],
            GenerateError::UnsupportedType { detail, .. }
                if detail.contains("missing SpecOperation annotation")
        ));
    }

    #[test]
    fn generates_json_output() {
        let file = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "basic",
                    inputs([("a", number(1)), ("b", number(2))]),
                    expected([("outcome", string("Ok")), ("result", number(3))]),
                )],
            ),
            &[
                operation("calc", OperationKind::Stateless, "calc::add"),
                setup("calc", "default", "calc::setup"),
                capture("calc", "calc::Result::value"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains("SPECGATE_RESULTS_PATH"));
        assert!(file.content.contains("fn specgate_write_result"));
        assert!(file.content.contains("fn specgate_write_traces"));
        assert!(file.content.contains("specgate_write_result("));
        assert!(file.content.contains(
            "let (specgate_traces, specgate_traces_file) = specgate_write_traces(\"basic\");"
        ));
        assert!(file.content.contains("let specgate_traces_match = None;"));
        assert!(file.content.contains("let _setup = calc::setup(1, 2);"));
        assert!(file.content.contains("let actual = calc::add(1, 2);"));
    }

    #[test]
    fn writes_traces_file_when_runtime_traces_exist() {
        let file = generate_test_file(
            &spec_with_cases(
                "counting",
                vec![spec_case(
                    "increment_once",
                    BTreeMap::new(),
                    expected([("outcome", string("Ok")), ("count", number(1))]),
                )],
            ),
            &[
                operation(
                    "counting",
                    OperationKind::StateMachine,
                    "counter::Counter::increment",
                ),
                setup("counting", "default", "counter::make_counter"),
                capture("counting", "counter::Counter::count"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("PathBuf::from(\"target\").join(\"specgate-harness\").join(\"traces\")")
        );
        assert!(
            file.content
                .contains("result[\"traces_file\"] = serde_json::Value::String(traces_file);")
        );
        assert!(
            file.content
                .contains("result[\"traces_match\"] = serde_json::Value::Bool(traces_match);")
        );
        assert!(file.content.contains("let specgate_status = \"pass\";"));
    }

    #[test]
    fn writes_trace_match_when_expected_traces_exist() {
        let file = generate_test_file(
            &spec_with_cases(
                "counting",
                vec![spec_case(
                    "increment_once",
                    BTreeMap::new(),
                    expected([
                        ("outcome", string("Ok")),
                        ("count", number(1)),
                        (
                            "traces",
                            Value::Sequence(vec![
                                json_mapping([(
                                    "OperationEnter",
                                    json_mapping([
                                        ("operation", string("counting")),
                                        ("symbol", string("counter::Counter::increment")),
                                    ]),
                                )]),
                                json_mapping([(
                                    "CaptureBefore",
                                    json_mapping([
                                        ("operation", string("counting")),
                                        ("field", string("count")),
                                        ("value", string("0")),
                                    ]),
                                )]),
                            ]),
                        ),
                    ]),
                )],
            ),
            &[
                operation(
                    "counting",
                    OperationKind::StateMachine,
                    "counter::Counter::increment",
                ),
                setup("counting", "default", "counter::make_counter"),
                capture("counting", "counter::Counter::count"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains("let specgate_traces_match = Some("));
        assert!(
            file.content
                .contains("serde_json::to_value(&specgate_traces)")
        );
        assert!(
            file.content.contains(
                "let specgate_status = if specgate_traces_match == Some(false) { \"fail\" } else { \"pass\" };"
            )
        );
    }

    #[test]
    fn non_default_setup_is_used_when_default_is_absent() {
        let file = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "basic",
                    inputs([("a", number(1))]),
                    expected([("outcome", string("Ok")), ("result", number(1))]),
                )],
            ),
            &[
                operation("calc", OperationKind::Stateless, "calc::add"),
                setup("calc", "runtime", "calc::setup_runtime"),
                capture("calc", "calc::Result::result"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("// setup runtime via calc::setup_runtime")
        );
        assert!(
            file.content
                .contains("let _setup = calc::setup_runtime(1);")
        );
    }

    #[test]
    fn method_with_no_inputs_avoids_extra_comma() {
        let file = generate_test_file(
            &spec_with_cases(
                "ping",
                vec![spec_case(
                    "basic",
                    BTreeMap::new(),
                    expected([("outcome", string("Ok")), ("status", string("up"))]),
                )],
            ),
            &[
                operation("ping", OperationKind::Stateless, "svc::Pinger::ping"),
                setup("ping", "default", "svc::setup_pinger"),
                capture("ping", "svc::PingResult::status"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let mut subject = svc::setup_pinger();")
        );
        assert!(
            file.content
                .contains("let actual = svc::Pinger::ping(&mut subject);")
        );
    }

    #[test]
    fn inline_checkpoint() {
        let file = generate_test_file(
            &spec_with_cases(
                "pipeline",
                vec![spec_case(
                    "with_inline",
                    inputs([("input", string("hello"))]),
                    expected([
                        ("outcome", string("Ok")),
                        ("checkpoints", Value::Sequence(vec![string("count_42")])),
                    ]),
                )],
            ),
            &[
                operation("pipeline", OperationKind::Sequence, "pipe::process"),
                setup("pipeline", "default", "pipe::setup"),
                checkpoint("pipeline", "pipe::process::checkpoint_1"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains("pipe::process::checkpoint_1"));
        assert!(
            file.content
                .contains("assert_eq!(checkpoints, vec![\"count_42\"]);")
        );
    }

    #[test]
    fn api_target_simple() {
        let file = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "api",
                vec![spec_case(
                    "single_pass",
                    inputs([("spec_path", string("fixtures/simple_pass.spec.yaml"))]),
                    expected([
                        ("outcome", string("Complete")),
                        (
                            "report",
                            json_mapping([
                                ("passed", number(1)),
                                ("failed", number(0)),
                                ("total", number(1)),
                            ]),
                        ),
                    ]),
                )],
            ),
            &[],
            Some(&api_binding_target(
                "specgate_harness::Harness::run_spec",
                Some("specgate_harness::Harness::new"),
            )),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let mut subject = specgate_harness::Harness::new(repo_root);")
        );
        assert!(
            file.content
                .contains("let actual = subject.run_spec(\"fixtures/simple_pass.spec.yaml\");")
        );
        assert!(file.content.contains("serde_json::to_value(&actual)"));
        assert!(
            file.content
                .contains("assert_eq!(actual_json[\"outcome\"], json!(\"Complete\"));")
        );
        assert!(
            file.content
                .contains("assert_eq!(actual_json[\"report\"][\"passed\"], json!(1));")
        );
    }

    #[test]
    fn api_target_error_variant() {
        let file = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "api",
                vec![spec_case(
                    "spec_not_found",
                    inputs([("spec_path", string("fixtures/nonexistent.spec.yaml"))]),
                    expected([
                        ("outcome", string("Error")),
                        (
                            "error",
                            json_mapping([(
                                "SpecNotFound",
                                json_mapping([("path", string("fixtures/nonexistent.spec.yaml"))]),
                            )]),
                        ),
                    ]),
                )],
            ),
            &[],
            Some(&api_binding_target(
                "specgate_harness::Harness::run_spec",
                Some("specgate_harness::Harness::new"),
            )),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("assert_eq!(actual_json[\"outcome\"], json!(\"Error\"));")
        );
        assert!(file.content.contains(
            "assert_eq!(actual_json[\"error\"][\"SpecNotFound\"][\"path\"], json!(\"fixtures/nonexistent.spec.yaml\"));"
        ));
    }

    #[test]
    fn api_target_multiple_cases() {
        let file = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "api",
                vec![
                    spec_case(
                        "pass_case",
                        inputs([("spec_path", string("fixtures/simple_pass.spec.yaml"))]),
                        expected([
                            ("outcome", string("Complete")),
                            (
                                "report",
                                json_mapping([("passed", number(1)), ("total", number(1))]),
                            ),
                        ]),
                    ),
                    spec_case(
                        "fail_case",
                        inputs([("spec_path", string("fixtures/simple_fail.spec.yaml"))]),
                        expected([
                            ("outcome", string("Complete")),
                            (
                                "report",
                                json_mapping([
                                    ("passed", number(0)),
                                    ("failed", number(1)),
                                    ("total", number(1)),
                                ]),
                            ),
                        ]),
                    ),
                    spec_case(
                        "not_found",
                        inputs([("spec_path", string("fixtures/nonexistent.spec.yaml"))]),
                        expected([
                            ("outcome", string("Error")),
                            ("error", json_mapping([("SpecNotFound", json_mapping([]))])),
                        ]),
                    ),
                ],
            ),
            &[],
            Some(&api_binding_target(
                "specgate_harness::Harness::run_spec",
                Some("specgate_harness::Harness::new"),
            )),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains("fn pass_case()"));
        assert!(file.content.contains("fn fail_case()"));
        assert!(file.content.contains("fn not_found()"));
    }

    #[test]
    fn command_target_simple() {
        let file = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "test",
                vec![spec_case(
                    "single_pass",
                    inputs([("spec_path", string("fixtures/simple_pass.spec.yaml"))]),
                    expected([
                        ("outcome", string("Complete")),
                        (
                            "report",
                            json_mapping([("passed", number(1)), ("total", number(1))]),
                        ),
                    ]),
                )],
            ),
            &[],
            Some(&command_binding_target(
                "cargo test -p specgate-harness {spec_path}",
                Some("{workdir}/results.json"),
            )),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("specgate_spawn_shell_command(&command_line)")
        );
        assert!(file.content.contains(
            "let command_line = specgate_apply_template(\"cargo test -p specgate-harness {spec_path}\", &replacements);"
        ));
        assert!(file.content.contains(
            "let output_path = specgate_apply_template(\"{workdir}/results.json\", &replacements);"
        ));
        assert!(
            file.content
                .contains("let spec_path_path = case_dir.join(\"spec_path.txt\");")
        );
        assert!(
            file.content.contains(
                "replacements.push((\"spec_path\", spec_path_path.display().to_string()));"
            )
        );
        assert!(
            file.content
                .contains("let actual_json: serde_json::Value = serde_json::from_str(&stdout)")
        );
        assert!(
            file.content
                .contains("assert_eq!(actual_json[\"report\"][\"passed\"], json!(1));")
        );
    }

    #[test]
    fn command_target_stdout_output() {
        let file = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "test",
                vec![spec_case(
                    "single_pass",
                    inputs([("spec_path", string("fixtures/simple_pass.spec.yaml"))]),
                    expected([("outcome", string("Complete"))]),
                )],
            ),
            &[],
            Some(&command_binding_target(
                "cargo test -p specgate-harness {spec_path}",
                None,
            )),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let stdout = String::from_utf8(output.stdout)")
        );
    }

    #[test]
    fn command_target_missing_command() {
        let error = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "test",
                vec![spec_case("basic", BTreeMap::new(), expected([]))],
            ),
            &[],
            Some(&BindingTarget {
                package_root: "specgate-cli".to_string(),
                test_root: None,
                build: None,
                command: None,
                function: None,
                constructor: None,
                outputs: BindingTargetOutputs::default(),
            }),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert_eq!(
            error,
            vec![GenerateError::UnsupportedType {
                type_name: "binding_target".to_string(),
                detail: "binding target requires command or function field".to_string(),
            }]
        );
    }

    #[test]
    fn api_target_without_constructor_calls_function_directly() {
        let file = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "api",
                vec![spec_case(
                    "single_pass",
                    inputs([("spec_path", string("fixtures/simple_pass.spec.yaml"))]),
                    expected([("outcome", string("Complete"))]),
                )],
            ),
            &[],
            Some(&api_binding_target("specgate_harness::run_spec", None)),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(file.content.contains(
            "let actual = specgate_harness::run_spec(\"fixtures/simple_pass.spec.yaml\");"
        ));
    }

    #[test]
    fn api_target_missing_function() {
        let error = generate_test_file(
            &spec_with_target_and_cases(
                "harness.core",
                "api",
                vec![spec_case(
                    "basic",
                    inputs([("spec_path", string("fixtures/simple_pass.spec.yaml"))]),
                    expected([("outcome", string("Complete"))]),
                )],
            ),
            &[],
            Some(&api_binding_target("", None)),
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert_eq!(
            error,
            vec![GenerateError::UnsupportedType {
                type_name: "binding_target".to_string(),
                detail: "binding target requires command or function field".to_string(),
            }]
        );
    }

    #[test]
    fn render_json_expectations_rejects_non_string_mapping_keys() {
        let mut expected = BTreeMap::new();
        let mut nested = Mapping::new();
        nested.insert(number(1), number(2));
        expected.insert("error".to_string(), Value::Mapping(nested));

        let error = render_json_expectations(&mut String::new(), "actual_json", &expected)
            .expect_err("render should fail");
        assert!(matches!(
            error,
            GenerateError::UnsupportedType { detail, .. } if detail.contains("mapping keys must be strings")
        ));
    }

    #[test]
    fn render_json_value_supports_common_shapes() {
        assert_eq!(
            render_json_value(&Value::Null).expect("null should render"),
            "json!(null)"
        );
        assert_eq!(
            render_json_value(&boolean(true)).expect("bool should render"),
            "json!(true)"
        );
        assert_eq!(
            render_json_value(&Value::Sequence(vec![number(1), string("two")]))
                .expect("sequence should render"),
            "json!([1, \"two\"])"
        );
        assert_eq!(
            render_json_value(&json_mapping([("ok", number(1))])).expect("mapping should render"),
            "json!({\"ok\": 1})"
        );
    }

    #[test]
    fn render_json_value_rejects_bad_mappings_and_tags() {
        let mut mapping = Mapping::new();
        mapping.insert(number(1), number(2));

        let mapping_error =
            render_json_value(&Value::Mapping(mapping)).expect_err("render should fail");
        assert!(matches!(
            mapping_error,
            GenerateError::UnsupportedType { detail, .. } if detail.contains("mapping keys must be strings")
        ));

        let tagged = serde_yaml::from_str::<Value>("!json 1").expect("tagged value should parse");
        let tagged_error = render_json_value(&tagged).expect_err("render should fail");
        assert!(matches!(
            tagged_error,
            GenerateError::UnsupportedType { detail, .. } if detail.contains("tagged YAML values")
        ));
    }

    #[test]
    fn apply_case_template_supports_scalars_and_skips_mock_inputs() {
        let rendered = apply_case_template(
            "run {spec_path} {enabled} {attempts}",
            &spec_case(
                "templated",
                inputs([
                    ("spec_path", string("fixtures/simple_pass.spec.yaml")),
                    ("enabled", boolean(true)),
                    ("attempts", number(3)),
                    ("mock_backend", string("ignored")),
                ]),
                expected([]),
            ),
        )
        .expect("template should render");

        assert_eq!(rendered, "run fixtures/simple_pass.spec.yaml true 3");
    }

    #[test]
    fn scalar_template_value_supports_null_and_rejects_complex_values() {
        assert_eq!(
            scalar_template_value(&Value::Null).expect("null should render"),
            "null"
        );

        let sequence_error = scalar_template_value(&Value::Sequence(vec![number(1)]))
            .expect_err("sequence should fail");
        assert!(matches!(
            sequence_error,
            GenerateError::UnsupportedType { detail, .. }
                if detail.contains("command template values must be scalar")
        ));

        let tagged =
            serde_yaml::from_str::<Value>("!template 1").expect("tagged value should parse");
        let tagged_error = scalar_template_value(&tagged).expect_err("tagged value should fail");
        assert!(matches!(
            tagged_error,
            GenerateError::UnsupportedType { detail, .. } if detail.contains("tagged YAML values")
        ));
    }

    #[test]
    fn capture_without_matching_expected_only_reads_the_value() {
        let file = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "basic",
                    inputs([("a", number(1))]),
                    expected([("outcome", string("Ok"))]),
                )],
            ),
            &[
                operation("calc", OperationKind::Stateless, "calc::add"),
                setup("calc", "default", "calc::setup"),
                capture("calc", "calc::Result::value"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(
            file.content
                .contains("let actual_value = actual.value.clone();")
        );
        assert!(!file.content.contains("assert_eq!(actual_value"));
    }

    #[test]
    fn capture_all_in_state_machine_skips_before_capture_lines() {
        let file = generate_test_file(
            &spec_with_cases(
                "machine",
                vec![spec_case(
                    "advance",
                    BTreeMap::new(),
                    expected([("outcome", string("Ok")), ("state", string("next"))]),
                )],
            ),
            &[
                operation(
                    "machine",
                    OperationKind::StateMachine,
                    "svc::Machine::advance",
                ),
                setup("machine", "default", "svc::setup_machine"),
                capture_all("machine", "svc::MachineState"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect("generation should succeed");

        assert!(!file.content.contains("before_state"));
        assert!(
            file.content
                .contains("let actual_state = subject.state.clone();")
        );
    }

    #[test]
    fn tagged_yaml_is_unsupported() {
        let tagged_value =
            serde_yaml::from_str::<Value>("!custom 1").expect("tagged value should parse");
        let mut mapping = Mapping::new();
        mapping.insert(Value::String("a".to_string()), tagged_value);

        let error = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "tagged_input",
                    BTreeMap::from([("payload".to_string(), Value::Mapping(mapping))]),
                    expected([("outcome", string("Ok")), ("result", number(1))]),
                )],
            ),
            &[
                operation("calc", OperationKind::Stateless, "calc::add"),
                setup("calc", "default", "calc::setup"),
                capture("calc", "calc::Result::value"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert!(matches!(
            &error[0],
            GenerateError::UnsupportedType { detail, .. } if detail.contains("tagged YAML values")
        ));
    }

    #[test]
    fn tagged_mock_input_is_unsupported() {
        let tagged_value =
            serde_yaml::from_str::<Value>("!mock 1").expect("tagged value should parse");

        let error = generate_test_file(
            &spec_with_cases(
                "fetch",
                vec![spec_case(
                    "bad_mock",
                    inputs([("mock_backend", tagged_value)]),
                    expected([("outcome", string("Ok"))]),
                )],
            ),
            &[
                operation("fetch", OperationKind::Stateless, "fetch::run"),
                setup("fetch", "default", "fetch::setup"),
                mock_annotation("fetch", "backend", "fetch::call_backend"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert!(matches!(
            &error[0],
            GenerateError::UnsupportedType { detail, .. } if detail.contains("tagged YAML values")
        ));
    }

    #[test]
    fn checkpoint_outputs_must_be_sequences() {
        let error = generate_test_file(
            &spec_with_cases(
                "pipeline",
                vec![spec_case(
                    "bad_checkpoint",
                    BTreeMap::new(),
                    expected([
                        ("outcome", string("Ok")),
                        ("checkpoints", string("not-a-list")),
                    ]),
                )],
            ),
            &[
                operation("pipeline", OperationKind::Sequence, "pipe::process"),
                setup("pipeline", "default", "pipe::setup"),
                checkpoint("pipeline", "pipe::process::checkpoint_1"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert!(matches!(
            &error[0],
            GenerateError::UnsupportedType { detail, .. }
                if detail.contains("expected a sequence for checkpoint assertions")
        ));
    }

    #[test]
    fn tagged_capture_expectation_is_unsupported() {
        let tagged_value =
            serde_yaml::from_str::<Value>("!expected 1").expect("tagged value should parse");

        let error = generate_test_file(
            &spec_with_cases(
                "calc",
                vec![spec_case(
                    "bad_expectation",
                    inputs([("a", number(1))]),
                    BTreeMap::from([
                        ("outcome".to_string(), string("Ok")),
                        ("result".to_string(), tagged_value),
                    ]),
                )],
            ),
            &[
                operation("calc", OperationKind::Stateless, "calc::add"),
                setup("calc", "default", "calc::setup"),
                capture("calc", "calc::Result::result"),
            ],
            None,
            Path::new("tests").join("specgate_generated.rs").as_path(),
            Path::new("target").join("results.json").as_path(),
        )
        .expect_err("generation should fail");

        assert!(matches!(
            &error[0],
            GenerateError::UnsupportedType { detail, .. } if detail.contains("tagged YAML values")
        ));
    }

    #[test]
    fn render_value_rejects_non_string_mapping_keys() {
        let mut mapping = Mapping::new();
        mapping.insert(number(1), number(2));

        let error = render_value(&Value::Mapping(mapping)).expect_err("render should fail");
        assert!(matches!(
            error,
            GenerateError::UnsupportedType { detail, .. } if detail.contains("mapping keys must be strings")
        ));
    }

    #[test]
    fn render_string_vec_rejects_non_sequence_values() {
        let error = render_string_vec(&string("nope")).expect_err("render should fail");
        assert!(matches!(
            error,
            GenerateError::UnsupportedType { detail, .. }
                if detail.contains("expected a sequence for checkpoint assertions")
        ));
    }

    #[test]
    fn render_value_supports_null_and_sequences() {
        assert_eq!(
            render_value(&Value::Null).expect("null should render"),
            "serde_json::Value::Null"
        );
        assert_eq!(
            render_value(&Value::Sequence(vec![number(1), string("two")]))
                .expect("sequence should render"),
            "vec![1, \"two\"]"
        );
    }

    #[test]
    fn sanitize_test_name_replaces_non_identifiers() {
        assert_eq!(sanitize_test_name("hello-world"), "hello_world");
    }

    fn spec_with_cases(name: &str, cases: Vec<SpecCase>) -> SpecDocument {
        spec_with_target_and_cases(name, "test", cases)
    }

    fn spec_with_target_and_cases(name: &str, target: &str, cases: Vec<SpecCase>) -> SpecDocument {
        SpecDocument {
            name: name.to_string(),
            binding: Some(BindingDecl::Single(BindingEntry {
                name: "rust".to_string(),
                target: target.to_string(),
            })),
            depends_on: Vec::new(),
            state: BTreeMap::new(),
            init: BTreeMap::new(),
            operations: BTreeMap::new(),
            invariants: BTreeMap::new(),
            inputs: BTreeMap::new(),
            types: BTreeMap::new(),
            outcome: Value::String("Ok".to_string()),
            outputs: BTreeMap::new(),
            cases,
        }
    }

    fn spec_case(
        name: &str,
        inputs: BTreeMap<String, Value>,
        expected: BTreeMap<String, Value>,
    ) -> SpecCase {
        SpecCase {
            name: name.to_string(),
            desc: format!("case {name}"),
            binding: None,
            inputs,
            expected,
            steps: Vec::new(),
            postconditions: None,
        }
    }

    fn operation(operation: &str, kind: OperationKind, symbol: &str) -> Annotation {
        Annotation::SpecOperation {
            operation: operation.to_string(),
            kind,
            symbol: symbol.to_string(),
        }
    }

    fn setup(operation: &str, name: &str, symbol: &str) -> Annotation {
        Annotation::SpecSetup {
            operation: operation.to_string(),
            name: name.to_string(),
            symbol: symbol.to_string(),
            params: Vec::new(),
            returns: None,
        }
    }

    fn capture(operation: &str, symbol: &str) -> Annotation {
        Annotation::SpecCapture {
            operation: operation.to_string(),
            symbol: symbol.to_string(),
            capture_all: false,
        }
    }

    fn capture_all(operation: &str, symbol: &str) -> Annotation {
        Annotation::SpecCapture {
            operation: operation.to_string(),
            symbol: symbol.to_string(),
            capture_all: true,
        }
    }

    fn checkpoint(operation: &str, symbol: &str) -> Annotation {
        Annotation::SpecCheckpoint {
            operation: operation.to_string(),
            symbol: symbol.to_string(),
        }
    }

    fn mock_annotation(operation: &str, name: &str, symbol: &str) -> Annotation {
        Annotation::SpecMock {
            operation: operation.to_string(),
            name: name.to_string(),
            symbol: symbol.to_string(),
        }
    }

    fn api_binding_target(function: &str, constructor: Option<&str>) -> BindingTarget {
        BindingTarget {
            package_root: "specgate-harness".to_string(),
            test_root: None,
            build: None,
            command: None,
            function: if function.is_empty() {
                None
            } else {
                Some(function.to_string())
            },
            constructor: constructor.map(ToString::to_string),
            outputs: BindingTargetOutputs::default(),
        }
    }

    fn command_binding_target(command: &str, output_file: Option<&str>) -> BindingTarget {
        BindingTarget {
            package_root: "specgate-cli".to_string(),
            test_root: None,
            build: None,
            command: Some(command.to_string()),
            function: None,
            constructor: None,
            outputs: BindingTargetOutputs {
                file: output_file.map(ToString::to_string),
                stdout: None,
            },
        }
    }

    fn inputs<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
        entries
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect()
    }

    fn expected<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
        inputs(entries)
    }

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn boolean(value: bool) -> Value {
        Value::Bool(value)
    }

    fn number(value: i64) -> Value {
        serde_yaml::to_value(value).expect("number should serialize")
    }

    fn json_mapping<const N: usize>(entries: [(&str, Value); N]) -> Value {
        let mut mapping = Mapping::new();
        for (key, value) in entries {
            mapping.insert(Value::String(key.to_string()), value);
        }
        Value::Mapping(mapping)
    }
}
