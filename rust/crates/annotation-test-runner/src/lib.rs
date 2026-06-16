use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use specgate::{CompileError, extract_annotations};

pub fn run(
    case_name: Option<&str>,
    source_arg: &str,
    driver_arg: Option<&str>,
    current_dir: &Path,
) -> Result<Value, String> {
    if let Some(driver_arg) =
        driver_arg.filter(|driver| !driver.is_empty() && *driver != "{driver}")
    {
        let case_name = case_name.unwrap_or("runtime_case");
        return annotation_trace_runner::run(case_name, source_arg, driver_arg);
    }

    let source = read_source_arg(source_arg)?;
    match extract_annotations(&source, "fixture") {
        Ok(annotations) => {
            let registry_dir = current_dir.join("target").join("specgate");
            fs::create_dir_all(&registry_dir)
                .map_err(|error| format!("failed to create {}: {error}", registry_dir.display()))?;
            let registry_path = registry_dir.join("annotations.json");
            fs::write(
                &registry_path,
                serde_json::to_string(&annotations).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("failed to write {}: {error}", registry_path.display()))?;
            let _ = case_name;
            Ok(json!({
                "outcome": "Ok",
                "output_file": "target/specgate/annotations.json",
                "annotations": render_annotations(&annotations),
            }))
        }
        Err(errors) => Ok(json!({
            "outcome": "Error",
            "errors": render_errors(&errors),
        })),
    }
}

fn read_source_arg(source_arg: &str) -> Result<String, String> {
    let path = PathBuf::from(source_arg);
    if path.is_file() {
        fs::read_to_string(path).map_err(|error| error.to_string())
    } else {
        Ok(source_arg.to_string())
    }
}

fn render_errors(errors: &[CompileError]) -> Vec<Value> {
    errors
        .iter()
        .map(|error| json!({ "CompileError": { "message": error.message } }))
        .collect()
}

fn render_annotations(annotations: &[specgate::Annotation]) -> Vec<Value> {
    annotations
        .iter()
        .map(|annotation| {
            let mut value = serde_json::to_value(annotation).expect("annotation should serialize");
            strip_capture_all(&mut value);
            value
        })
        .collect()
}

fn strip_capture_all(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("capture_all");
            for child in map.values_mut() {
                strip_capture_all(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_capture_all(child);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}
