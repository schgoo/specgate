// Operation with no return value (unit type).
use specgate_annotations::*;

#[spec_setup("make_logger")]
pub fn make_logger() -> Logger {
    Logger { count: 0 }
}

#[derive(SpecEvent)]
pub struct Logger {
    #[spec_event]
    pub count: i32,
}

impl Logger {
    #[spec_operation("log")]
    pub fn log(&mut self, msg: &str) {
        self.count += 1;
    }
}
