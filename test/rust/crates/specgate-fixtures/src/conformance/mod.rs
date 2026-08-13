//! Conformance fixtures — canonical operation-shape examples, dual-bound
//! Rust<->C#, that must emit byte-identical canonical traces across every bound
//! target. Grouped by shape category, mirrored by the C# `Conformance/<Category>/`
//! tree. Exercised by the `specgate.conformance` spec.
pub mod r#async;
pub mod basic;
pub mod complex_inputs;
pub mod external_dep;
pub mod mocks;
pub mod multi_file;
pub mod stateful;
pub mod structured;
pub mod sum_types;
pub mod witness;
