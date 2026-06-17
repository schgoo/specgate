// Operation with no return value (unit type).
use specgate_annotations::*;

#[spec_setup("make_logger")]
fn make_logger() -> Logger {
    Logger { count: 0 }
}

struct Logger {
    #[spec_event]
    count: i32,
}

impl Logger {
    #[spec_operation("log")]
    fn log(&mut self, msg: &str) {
        self.count += 1;
    }
}
