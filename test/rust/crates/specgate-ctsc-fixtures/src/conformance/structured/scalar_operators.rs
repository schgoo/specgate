use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.scalar_operators")]
pub struct Measurement {
    #[spec_event]
    pub temperature: i32,
    #[spec_event]
    pub label: String,
    #[spec_event]
    pub readings: Vec<i32>,
}

#[spec_operation("get_measurement", spec = "fixture.scalar_operators")]
pub fn get_measurement() -> Measurement {
    Measurement {
        temperature: 72,
        label: "sensor-A3-north".to_string(),
        readings: vec![68, 70, 72, 71, 73],
    }
}

#[spec_operation("get_empty", spec = "fixture.scalar_operators")]
pub fn get_empty() -> Vec<String> {
    Vec::new()
}

#[test]
fn scalar_values_are_asserted_without_matcher_dsl() {
    let measurement = get_measurement();
    assert!(measurement.temperature > 70);
    assert!(measurement.label.starts_with("sensor-"));
    assert_eq!(measurement.readings.len(), 5);
    assert!(get_empty().is_empty());
}
