// Operation with no return value (unit type).
use specgate::*;

#[spec_setup("log")]
pub fn make_logger() -> Logger {
    Logger { count: 0 }
}

#[derive(SpecEvent)]
pub struct Logger {
    #[spec_event]
    pub count: i32,
}

impl Logger {
    #[spec_operation("log", spec = "fixture.void_operation")]
    pub fn log(&mut self, #[spec_input("msg")] _msg: &str) {
        self.count += 1;
    }
}
