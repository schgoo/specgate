//! Operations for one component split across multiple files.

#[path = "../../../../../../cases/fixture.multi_file/rust/farewell.rs"]
pub mod farewell;
#[path = "../../../../../../cases/fixture.multi_file/rust/greet.rs"]
pub mod greet;

pub use farewell::farewell;
pub use greet::greet;
