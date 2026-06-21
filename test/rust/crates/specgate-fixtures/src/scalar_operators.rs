// Scalar operators — numeric comparisons, regex, negation.
use specgate::*;

#[derive(SpecEvent)]
pub struct Measurement {
    #[spec_event(name = "temperature")]
    pub temperature: i32,
    #[spec_event(name = "label")]
    pub label: String,
    #[spec_event(name = "readings")]
    pub readings: Vec<i32>,
}

#[spec_operation("get_measurement")]
pub fn get_measurement() -> Measurement {
    Measurement {
        temperature: 72,
        label: "sensor-A3-north".to_string(),
        readings: vec![68, 70, 72, 71, 73],
    }
}

#[spec_operation("get_empty")]
pub fn get_empty() -> Vec<String> {
    vec![]
}
