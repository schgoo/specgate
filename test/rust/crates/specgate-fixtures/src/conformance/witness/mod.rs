//! Cross-target divergence witnesses — deliberately construct a difference so
//! the harness must report it; guards against a vacuous conformance pass.
pub mod divergence_witness;

/// The real-build witness operation is emitted by `build.rs` into `OUT_DIR`, so
/// it exists only in the compiled crate (no source file on disk). It is the Rust
/// analog of the C# Roslyn witness generator: a source-scanning runner cannot
/// see it, so running it proves the harness executes the fixture's real built
/// artifact rather than a source-reconstructed surrogate.
pub mod generated {
    use specgate::*;
    include!(concat!(env!("OUT_DIR"), "/generated_witness.rs"));
}
