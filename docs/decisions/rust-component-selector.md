# Rust component-selector spelling

> **ADR:** ADR-0021
> **Status:** accepted
> **Owner:** @schgoo (user-ratified)
> **Date:** 2026-11-17

## Context

The Rust `spec_operation` and `spec_setup` attributes used `spec` to select a
component, not a specification file. Issue #147 requires a clearer name and an
explicit compatibility decision.

## Decision

Both attributes use `component = "..."`. The owner chose to reject the old
`spec = "..."` spelling, rather than retain it as a deprecated alias.
Diagnostics direct callers to `component`.

Omitting the selector still uses the crate-root `spec_component!` declaration.
Repeated `component` arguments reject, including identical values. Mixing the
old and new spellings rejects in either order.

## Consequences

Existing explicit Rust selectors must be renamed. C# `Spec` properties are
unchanged. Component identities, setup matching, capture behavior, and CTSC
document shapes and artifacts do not change.

## Validation

Macro parser tests cover explicit/default selection, duplicate selectors, and
old or mixed spellings. Native capture tests cover explicit operation and setup
selection and crate-default fallback. The full golden check must pass without
changing checked-in CTSC artifacts.
