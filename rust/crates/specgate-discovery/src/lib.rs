//! Native implementation discovery for `SpecGate`'s CTSC workflow.
//!
//! This crate owns strict target binding, Rust link-time metadata discovery,
//! C# compiled-assembly reflection, raw invocation metadata, and deterministic
//! semantic schema normalization. The four crate-root `discover_*` functions
//! are the canonical primary workflow. Supporting APIs and result models use
//! cohesive module paths:
//!
//! - [`binding`] parses and resolves target bindings;
//! - [`identity`] defines strongly typed semantic identities;
//! - [`output`] exposes workflow result models;
//! - [`registry`] parses and queries raw discovery registries;
//! - [`schema`] normalizes semantic schemas;
//! - [`types`] and [`setup`] expose schema type and setup helpers;
//! - [`runner`] provides generated-runner Cargo and execution support;
//! - the `test-util` feature exposes [`test_util`] failure-injection harnesses.
//!
//! A caller can resolve a binding with [`binding::resolve_target`] and
//! pass it to [`discover_resolved`]. The example compiles without
//! invoking Cargo or requiring a fixture at documentation-test time.
//!
//! # Examples
//!
//! ```no_run
//! use specgate_discovery::{binding, discover_resolved};
//!
//! # fn example() -> Result<(), specgate_discovery::Error> {
//! let target = binding::resolve_target("binding.yaml", None)?;
//! let component = specgate_discovery::identity::ComponentId::from("com.example.component");
//! let discovered = discover_resolved(target, &component)?;
//! println!("{}", discovered.schema.component);
//! # Ok(())
//! # }
//! ```

mod csharp_discovery;
mod discovery;
/// Structured discovery errors and stable failure classification.
mod error;
/// Strong semantic identities used throughout discovery.
pub mod identity;

/// Strict target-binding parsing and resolution.
pub mod binding;
/// Generated-runner Cargo identity and execution support.
pub mod runner;
/// Downstream binding and discovery failure-injection harnesses.
#[cfg(feature = "test-util")]
pub mod test_util;

#[doc(inline)]
pub use discovery::{discover, discover_batch, discover_many, discover_resolved};
#[doc(inline)]
pub use error::{Error, ErrorKind};

pub mod output {
    //! Result models returned by discovery workflows.

    #[doc(inline)]
    pub use crate::discovery::model::{Batch, Component, SchemaLookup, Target};
}
#[cfg(feature = "raw-registry")]
pub mod registry {
    //! Raw metadata emitted by native or reflection-based discovery.
    //!
    //! Parse a document with [`Registry::parse`], then query operations, setups,
    //! types, or present components without depending on the wire JSON model.
    #[doc(inline)]
    pub use crate::discovery::registry::{
        ExceptionMetadata, Field, Operation, OperationBuilder, OperationId, Registry, Type, TypeBuilder, TypeId, Variant,
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
        DependencySchema, ErrorDeclaration, Field, Input, Operation, OperationBuilder, Schema, Setup, TypeDeclaration, TypeKind,
        UnknownKind, Variant,
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
    pub use crate::discovery::setup::{MappedInput, SetupInput, build_inputs, raw_inputs};
}
