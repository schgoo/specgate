//! Deterministic semantic normalization of raw discovery metadata.
//!
//! [`dependency`] computes transitive component ownership and qualifies named
//! references. [`schema`] constructs language-neutral operation and type models
//! for Rust and pre-normalized C# input. [`validation`] rejects malformed or
//! unsupported surfaces before they reach CTSC encoding.

mod dependency;
mod schema;
mod validation;

pub(super) use schema::{build_normalized, build_schema};
pub(super) use validation::{reject_dynamic, validate_surface};

/// Source syntax used while constructing a canonical semantic schema.
#[derive(Clone, Copy)]
pub(super) enum TypeSyntax {
    /// Parse and map native Rust type syntax.
    Rust,
    /// Preserve an already-normalized CTSC type reference.
    Normalized,
}
