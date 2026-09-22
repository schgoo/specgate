use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.void_operation")]
pub struct Logger {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("log", spec = "fixture.void_operation")]
pub fn make_logger() -> Logger {
    Logger { count: 0 }
}

impl Logger {
    #[spec_operation("log", spec = "fixture.void_operation")]
    pub fn log(&mut self, #[spec_input("msg")] _message: &str) {
        self.count += 1;
        observe_count(self.count);
    }
}

#[spec_operation("observe_count", spec = "fixture.void_operation")]
pub fn observe_count(count: i32) -> i32 {
    count
}

#[test]
fn unit_operation_mutates_receiver() {
    let mut logger = make_logger();
    logger.log("captured");
    assert_eq!(logger.count, 1);
}
