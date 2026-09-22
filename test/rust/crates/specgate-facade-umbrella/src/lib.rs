//! Compile and capture fixture for the umbrella facade.

use sg::{spec_component, spec_operation};

spec_component!("fixture.facade_umbrella");

#[spec_operation("double")]
pub fn double(value: i32) -> i32 {
    value * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umbrella_facade_doubles_its_input() {
        assert_eq!(double(2), 4);
    }

    #[test]
    fn umbrella_facade_captures() {
        sg::__rt::start_native_capture(sg::__rt::NativeCaptureConfig {
            scenario_name: "umbrella".to_string(),
            trace_id: "77777777777777777777777777777777".to_string(),
            run_span_id: "7777777777777701".to_string(),
            scenario_span_id: "7777777777777702".to_string(),
            operation_span_ids: Vec::new(),
            start_time_unix_nano: 1,
            clock_step_unix_nano: 1,
        })
        .unwrap();
        assert_eq!(double(2), 4);
        let capture = sg::__rt::finish_native_capture().unwrap();
        assert_eq!(
            capture.operations[0].component_id,
            "fixture.facade_umbrella"
        );
    }
}
