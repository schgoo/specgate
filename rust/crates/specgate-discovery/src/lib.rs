//! Native implementation discovery for `SpecGate`'s CTSC workflow.
//!
//! This crate owns strict implementation binding, Rust link-time metadata discovery,
//! C# compiled-assembly reflection, raw invocation metadata, and deterministic
//! semantic schema normalization. The four crate-root `discover_*` functions
//! are the canonical primary workflow. Supporting APIs and result models use
//! cohesive module paths:
//!
//! - [`binding`] parses and resolves implementation bindings;
//! - [`identity`] defines strongly typed semantic identities;
//! - [`output`] exposes workflow result models;
//! - [`registry`] parses and queries raw discovery registries;
//! - [`schema`] normalizes semantic schemas;
//! - [`types`] and [`setup`] expose schema type and setup helpers;
//! - [`runner`] provides generated-runner Cargo and execution support;
//! - the `test-util` feature exposes [`test_util`] failure-injection harnesses.
//!
//! A caller can resolve a component implementation with
//! [`binding::resolve_implementation`] and pass it to [`discover_resolved`].
//! Discovery verifies that the selected sources actually declare the component.
//! The example compiles without invoking Cargo or requiring a fixture at
//! documentation-test time.
//!
//! # Examples
//!
//! ```no_run
//! use specgate_discovery::{binding, discover_resolved};
//!
//! # fn example() -> Result<(), specgate_discovery::Error> {
//! let implementation = binding::resolve_implementation("specgate.yaml", "com.example.component", None)?;
//! let discovered = discover_resolved(implementation)?;
//! println!("{}", discovered.schema.component);
//! # Ok(())
//! # }
//! ```

mod csharp_discovery;
mod discovery;
/// Opaque discovery errors with contextual source chains.
mod error;
/// Strong semantic identities used throughout discovery.
pub mod identity;

/// Strict implementation-binding parsing and resolution.
pub mod binding;
/// Generated-runner Cargo identity and execution support.
pub mod runner;
/// Downstream binding and discovery failure-injection harnesses.
#[cfg(feature = "test-util")]
pub mod test_util;

#[doc(inline)]
pub use discovery::{discover, discover_batch, discover_built, discover_many, discover_resolved};
#[doc(inline)]
pub use error::Error;

pub mod output {
    //! Result models returned by discovery workflows.

    #[doc(inline)]
    pub use crate::discovery::model::{Batch, Component, Discovered, SchemaLookup};
}
#[cfg(feature = "raw-registry")]
pub mod registry {
    //! Raw metadata emitted by native or reflection-based discovery.
    //!
    //! Parse a document with [`Registry::parse`], then query operations, setups,
    //! types, or present components without depending on the wire JSON model.
    #[doc(inline)]
    pub use crate::discovery::registry::{
        ExceptionMetadata, Field, Operation, OperationBuilder, OperationId, Registry, TestTarget, Type, TypeBuilder, TypeId, Variant,
    };
    #[doc(inline)]
    pub use crate::discovery::registry_json;
}

pub mod schema {
    //! Language-neutral semantic schemas and deterministic normalization.
    //!
    //! Normalization validates the requested registry surface and visibility,
    //! folds setup inputs, resolves dependency-owned types, and preserves
    //! per-component failures during batched discovery.
    #[doc(inline)]
    pub use crate::discovery::model::{
        Dependency, ErrorDeclaration, Field, Input, Operation, OperationBuilder, Schema, Setup, TypeDef, TypeKind, UnknownKind, Variant,
    };
    #[doc(inline)]
    pub use crate::discovery::{normalize_registry, schema};
}

pub mod types {
    //! Rust type parsing and CTSC semantic type mapping helpers.
    //!
    //! Parsing remains intentionally permissive: unknown source types are
    //! retained as named semantic references rather than rejected.
    #[doc(inline)]
    pub use crate::discovery::types::{SpecType, builtin, collect_refs, is_unit, map, normalize, runtime_value};
}

pub mod setup {
    //! Setup selection and operation-input folding helpers.
    //!
    //! Setups are selected by exact component and operation identity; ordering
    //! and duplicate diagnostics match the main discovery workflow.
    #[doc(inline)]
    pub use crate::discovery::setup::{MappedInput, SetupBinding, SetupInput, build_inputs, raw_inputs, resolve_setups};
}
