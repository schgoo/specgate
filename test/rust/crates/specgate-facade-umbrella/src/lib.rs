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
        sg::__rt::start(
            sg::__rt::Config::builder(sg::__rt::ConfigDeps {
                scenario_name: "umbrella".to_string(),
                trace_id: sg::__rt::TraceId::try_from("77777777777777777777777777777777").unwrap(),
                run_span_id: sg::__rt::SpanId::try_from("7777777777777701").unwrap(),
                scenario_span_id: sg::__rt::SpanId::try_from("7777777777777702").unwrap(),
            })
            .start_time(1)
            .clock_step(1)
            .build()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(double(2), 4);
        let capture = sg::__rt::finish().unwrap();
        assert_eq!(
            capture.operations[0].component_id,
            "fixture.facade_umbrella"
        );
    }
}
