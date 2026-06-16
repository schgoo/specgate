use specgate_annotations::*;

#[derive(Clone, Debug, SpecCapture)]
pub struct TracedCounter {
    #[spec_capture("annotated.traces")]
    pub count: i64,
}

impl TracedCounter {
    #[must_use]
    pub fn new() -> Self {
        Self { count: 0 }
    }

    #[spec_operation("annotated.traces", kind = StateMachine)]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}

impl Default for TracedCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[spec_setup("annotated.traces", name = "default")]
#[must_use]
pub fn make_counter() -> TracedCounter {
    TracedCounter::new()
}
