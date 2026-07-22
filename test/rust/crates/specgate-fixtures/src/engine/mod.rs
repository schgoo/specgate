//! Engine fixtures — Rust-only test vehicles for the harness itself: matching
//! semantics, validation, error handling, property tests, and async-runtime
//! selection. These are not cross-language conformance examples; they exercise
//! harness mechanism and are exercised only by the `specgate.harness` spec.
pub mod anywhere_event;
pub mod async_smol_timer;
pub mod async_tokio_timer;
pub mod engine_minimal;
pub mod keyword_collision;
pub mod matching;
pub mod missing_operation;
pub mod missing_setup;
pub mod nonpublic_op;
pub mod property_add;
pub mod provenance_example;
pub mod resolver_a;
pub mod resolver_b;
pub mod resolver_conflict;
pub mod run_failure;
pub mod type_exact;
// pub mod compile_error;  // intentionally broken — syntax error
