mod annotations;
mod backend;
mod generator;

pub use annotations::{Annotation, OperationKind};
pub use backend::RustBackend;
pub use generator::{GenerateError, GeneratedFile, generate_test_file};
