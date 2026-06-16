#![allow(unused_variables)]
#![allow(clippy::all)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use serde_json::json;

fn specgate_results_path() -> PathBuf {
    std::env::var_os("SPECGATE_RESULTS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Users\\schgoo\\repos\\specgate\\rust\\crates\\specgate-harness\\..\\..\\..\\rust\\target\\specgate-harness\\annotated.traces\\run-1781634945431376500\\results.json"))
}

fn specgate_traces_path(name: &str) -> PathBuf {
    PathBuf::from("target").join("specgate-harness").join("traces").join(format!("{name}.json"))
}

fn specgate_write_traces(name: &str) -> (Vec<specgate::TraceEvent>, Option<String>) {
    let traces = specgate::runtime::drain_traces();
    if traces.is_empty() {
        return (traces, None);
    }
    let traces_path = specgate_traces_path(name);
    if let Some(parent) = traces_path.parent() {
        fs::create_dir_all(parent).expect("specgate traces directory should be creatable");
    }
    fs::write(
        &traces_path,
        serde_json::to_string_pretty(&traces).expect("specgate traces should serialize"),
    )
    .expect("specgate traces file should be writable");
    (traces, Some(traces_path.display().to_string().replace('\\', "/")))
}

fn specgate_write_result(name: &str, status: &str, duration_ms: i64, traces_file: Option<String>, traces_match: Option<bool>) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(specgate_results_path())
        .expect("specgate results file should be writable");
    let mut result = json!({
        "name": name,
        "status": status,
        "duration_ms": duration_ms,
    });
    if let Some(traces_file) = traces_file {
        result["traces_file"] = serde_json::Value::String(traces_file);
    }
    if let Some(traces_match) = traces_match {
        result["traces_match"] = serde_json::Value::Bool(traces_match);
    }
    writeln!(file, "{}", serde_json::to_string(&result).expect("specgate result line should serialize"))
        .expect("specgate result line should be written");
}

fn specgate_drain_checkpoints() -> Vec<String> {
    specgate::runtime::drain_checkpoints()
}

fn specgate_apply_template(template: &str, replacements: &[(&str, String)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in replacements {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
}

fn specgate_spawn_shell_command(command_line: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(command_line);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command.arg("-c").arg(command_line);
        command
    }
}

#[test]
fn increment_once() {
    specgate::runtime::reset();
    let specgate_started = Instant::now();
    // setup default via specgate_harness::traced_counter::make_counter
    let mut subject = specgate_harness::traced_counter::make_counter();
    let before_count = subject.count.clone();
    // operation specgate_harness::traced_counter::TracedCounter::increment
    let actual = specgate_harness::traced_counter::TracedCounter::increment(&mut subject);
    let after_count = subject.count.clone();
    assert_eq!(after_count, 1);
    let (specgate_traces, specgate_traces_file) = specgate_write_traces("increment_once");
    let specgate_traces_match = Some(serde_json::to_value(&specgate_traces)
        .expect("specgate traces should serialize") == json!([{"OperationEnter": {"operation": "annotated.traces", "symbol": "specgate_harness::traced_counter::TracedCounter::increment"}}, {"CaptureBefore": {"operation": "annotated.traces", "field": "count", "value": "0"}}, {"CaptureAfter": {"operation": "annotated.traces", "field": "count", "value": "1"}}, {"OperationExit": {"operation": "annotated.traces", "symbol": "specgate_harness::traced_counter::TracedCounter::increment"}}]));
    let specgate_status = if specgate_traces_match == Some(false) { "fail" } else { "pass" };
    specgate_write_result(
        "increment_once",
        specgate_status,
        specgate_started.elapsed().as_millis() as i64,
        specgate_traces_file,
        specgate_traces_match,
    );
}

