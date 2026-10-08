//! Select setup producers and fold their construction inputs into operation inputs.
//!
//! A setup with an explicit `fills` value replaces that operation parameter. A
//! setup without `fills` is matched by its normalized return type; exactly one
//! unmatched parameter selects parameter injection, while no match selects the
//! method receiver. Duplicate or ambiguous producers are rejected.
//!
//! # Examples
//! ```no_run
//! use specgate_discovery::{registry::{Operation, OperationId, Registry}, setup::{build_inputs, raw_inputs}};
//! let registry = Registry::parse(r#"{"operations":[],"types":[]}"#)?;
//! let operation = Operation::builder(OperationId::new("demo", "health")).build();
//! assert!(raw_inputs(&operation, &registry)?.is_empty());
//! assert!(build_inputs(&operation, &registry)?.is_empty());
//! # Ok::<(), specgate_discovery::Error>(())
//! ```
//!
//! Invalid or ambiguous setup metadata is returned as a normalization error by
//! both input functions rather than being silently ignored.

use super::{BTreeMap, BTreeSet};
use crate::discovery::registry::{Field, Operation, Registry};
use crate::discovery::types::{SpecType, map, normalize};
use crate::error::{Error, ErrorKind};
use crate::identity::{FieldName, TypeExpression};

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::Normalization, message)
}

// ---------------------------------------------------------------------------
// Inputs (setup-aware)
// ---------------------------------------------------------------------------

pub(super) struct FoldedSetup {
    pub(super) fills: FieldName,
    pub(super) inputs: Vec<Field>,
    pub(super) output: TypeExpression,
}

pub(super) struct FoldedOperation {
    pub(super) inputs: Vec<Field>,
    pub(super) setups: Vec<FoldedSetup>,
}

/// One setup-folded input retaining its source type expression.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "setup-folding DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct SetupInput {
    /// Semantic input name.
    pub name: FieldName,
    /// Source-language type expression.
    pub source_type: TypeExpression,
}

/// One setup-folded input mapped to a semantic type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "setup-folding DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct MappedInput {
    /// Semantic input name.
    pub name: FieldName,
    /// Mapped CTSC-facing type.
    pub value_type: SpecType,
}

/// Fold exact component-scoped setup producers into one operation's public
/// input surface.
///
/// # Errors
///
/// Rejects missing `fills` parameters, ambiguous type-based fills, multiple
/// receiver producers, duplicate fills, and duplicate folded input names.
pub(super) fn fold_operation(op: &Operation, registry: &Registry) -> Result<FoldedOperation, Error> {
    let setups = registry.setups_for(&op.component, &op.name);
    let mut param_injection: BTreeMap<FieldName, Vec<Field>> = BTreeMap::new();
    let mut receiver_setup: Option<&Operation> = None;
    let mut folded_setups = Vec::with_capacity(setups.len());

    for setup in setups {
        let target = if setup.fills.is_empty() {
            let mut candidates = op.params.iter().filter(|parameter| {
                !param_injection.contains_key(parameter.name.as_str()) && normalize(&parameter.ty) == normalize(&setup.return_type)
            });
            let first = candidates.next();
            match (first, candidates.next()) {
                (None, _) => None,
                (Some(parameter), None) => Some(parameter.name.clone()),
                (Some(first), Some(second)) => {
                    let names = std::iter::once(first)
                        .chain(std::iter::once(second))
                        .chain(candidates)
                        .map(|parameter| parameter.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(failure(format!(
                        "setup '{}' for '{}::{}' ambiguously matches parameters {names} by return type '{}'; set fills explicitly",
                        setup.fn_name, op.component, op.name, setup.return_type
                    )));
                }
            }
        } else {
            if !op.params.iter().any(|parameter| parameter.name == setup.fills) {
                return Err(failure(format!(
                    "setup '{}' for '{}::{}' fills unknown parameter '{}'",
                    setup.fn_name, op.component, op.name, setup.fills
                )));
            }
            Some(setup.fills.clone())
        };

        if let Some(target_name) = &target {
            if param_injection.insert(target_name.clone(), setup.params.clone()).is_some() {
                return Err(failure(format!(
                    "multiple setups for '{}::{}' fill parameter '{target_name}'",
                    op.component, op.name
                )));
            }
        } else if receiver_setup.replace(setup).is_some() {
            return Err(failure(format!(
                "multiple receiver setups are registered for '{}::{}'",
                op.component, op.name
            )));
        }
        folded_setups.push(FoldedSetup {
            fills: target.unwrap_or_default(),
            inputs: setup.params.clone(),
            output: setup.return_type.clone(),
        });
    }

    if op.is_method && receiver_setup.is_none() {
        return Err(failure(format!(
            "operation '{}::{}' is a method with no receiver setup; annotate a #[spec_setup(\"{}\")] producer for its receiver",
            op.component, op.name, op.name
        )));
    }

    let input_capacity = receiver_setup.as_ref().map_or(0, |setup| setup.params.len())
        + op.params
            .iter()
            .map(|parameter| param_injection.get(parameter.name.as_str()).map_or(1, Vec::len))
            .sum::<usize>();
    let mut inputs = Vec::with_capacity(input_capacity);
    if let Some(setup) = receiver_setup {
        inputs.extend(setup.params.iter().cloned());
    }
    for parameter in &op.params {
        if let Some(injected) = param_injection.get(parameter.name.as_str()) {
            inputs.extend(injected.iter().cloned());
        } else {
            inputs.push(parameter.clone());
        }
    }
    let mut names = BTreeSet::new();
    if let Some(duplicate) = inputs
        .iter()
        .map(|parameter| &parameter.name)
        .find(|name| !names.insert((*name).clone()))
    {
        return Err(failure(format!(
            "setup folding for '{}::{}' produces duplicate input name '{duplicate}'",
            op.component, op.name
        )));
    }
    folded_setups.sort_by(|left, right| left.fills.cmp(&right.fills));
    Ok(FoldedOperation {
        inputs,
        setups: folded_setups,
    })
}

/// Build an operation's raw setup-folded inputs.
///
/// # Errors
///
/// Returns setup ambiguity and invalid-metadata errors.
pub fn raw_inputs(op: &Operation, registry: &Registry) -> Result<Vec<SetupInput>, Error> {
    Ok(fold_operation(op, registry)?
        .inputs
        .into_iter()
        .map(|parameter| SetupInput {
            name: parameter.name,
            source_type: parameter.ty,
        })
        .collect())
}

/// Build an operation's normalized setup-folded inputs.
///
/// # Errors
///
/// Returns setup ambiguity and invalid-metadata errors.
pub fn build_inputs(op: &Operation, registry: &Registry) -> Result<Vec<MappedInput>, Error> {
    let type_names = registry.type_names();
    raw_inputs(op, registry)?
        .into_iter()
        .map(|input| {
            Ok(MappedInput {
                name: input.name,
                value_type: map(&input.source_type, &type_names)?,
            })
        })
        .collect()
}
