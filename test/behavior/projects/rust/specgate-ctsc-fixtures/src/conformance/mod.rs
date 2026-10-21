//! Capturable behavioral fixtures grouped by semantic feature.

pub mod r#async;
#[path = "basic/mod.rs"]
pub mod basic;
#[path = "complex_inputs/mod.rs"]
pub mod complex_inputs;
#[path = "dependencies/mod.rs"]
pub mod dependencies;
#[path = "multi_file/mod.rs"]
pub mod multi_file;
#[path = "stateful/mod.rs"]
pub mod stateful;
#[path = "structured/mod.rs"]
pub mod structured;
#[path = "sum_types/mod.rs"]
pub mod sum_types;
#[path = "translation/mod.rs"]
pub mod translation;
#[path = "witness/mod.rs"]
pub mod witness;
