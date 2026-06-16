use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use specgate::{Annotation, TraceEvent, create_runtime_fixture, extract_annotations};

pub fn run(case_name: &str, source_arg: &str, driver_arg: &str) -> Result<Value, String> {
    let source = read_arg(source_arg)?;
    let driver = read_arg(driver_arg)?;
    let fixture = create_runtime_fixture(case_name, &source, &driver)?;
    let stdout = fixture.run(case_feature_enabled(case_name))?;
    let mut traces: Vec<TraceEvent> = serde_json::from_str(&stdout)
        .map_err(|error| format!("failed to parse runtime traces: {error}; stdout={stdout}"))?;
    let annotations = extract_annotations(&source, "fixture").unwrap_or_default();
    normalize_symbols(&mut traces, &annotations);
    Ok(json!({
        "outcome": "Ok",
        "traces": traces,
    }))
}

fn read_arg(arg: &str) -> Result<String, String> {
    let path = PathBuf::from(arg);
    if path.is_file() {
        fs::read_to_string(path).map_err(|error| error.to_string())
    } else {
        Ok(arg.to_string())
    }
}

fn case_feature_enabled(case_name: &str) -> bool {
    case_name != "runtime_noop_without_feature"
}

fn normalize_symbols(traces: &mut [TraceEvent], annotations: &[Annotation]) {
    let symbols = annotation_symbol_map(annotations);
    for trace in traces {
        match trace {
            TraceEvent::OperationEnter { symbol, .. }
            | TraceEvent::OperationExit { symbol, .. }
            | TraceEvent::Checkpoint { symbol, .. } => {
                if let Some(full_symbol) = symbols.get(symbol) {
                    *symbol = full_symbol.clone();
                }
            }
            TraceEvent::CaptureAfter { .. }
            | TraceEvent::CaptureBefore { .. }
            | TraceEvent::MockCall { .. } => {}
        }
    }
}

fn annotation_symbol_map(annotations: &[Annotation]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for annotation in annotations {
        let symbol = match annotation {
            Annotation::SpecOperation { symbol, .. }
            | Annotation::SpecCheckpoint { symbol, .. } => symbol,
            _ => continue,
        };
        map.insert(short_symbol(symbol), symbol.clone());
    }
    map
}

fn short_symbol(symbol: &str) -> String {
    let parts = symbol.split("::").collect::<Vec<_>>();
    if parts.len() < 3 {
        return symbol.to_string();
    }
    if let Some(checkpoint) = parts.last().filter(|last| last.starts_with("checkpoint_")) {
        if parts.len() >= 4 {
            let mut shortened = parts[..parts.len() - 3].join("::");
            if !shortened.is_empty() {
                shortened.push_str("::");
            }
            shortened.push_str(parts[parts.len() - 2]);
            shortened.push_str("::");
            shortened.push_str(checkpoint);
            return shortened;
        }
    }
    let mut shortened = parts[..parts.len() - 2].join("::");
    if !shortened.is_empty() {
        shortened.push_str("::");
    }
    shortened.push_str(parts.last().expect("parts length checked"));
    shortened
}
