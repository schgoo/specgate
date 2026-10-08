# Opaque Rust errors

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-07

## Context

SpecGate's public Rust errors exposed `ErrorKind` enums, category predicates,
diagnostic accessors, and other internal state. That duplicated information
already available through `Display` and the standard source chain, coupled
callers to implementation taxonomy, and made routine context changes part of
the compatibility surface.

Some annotated CLI operations also record the workflow stage of a failure as
structured CTSC data. That stage is protocol data and must remain
machine-readable, but it is not the public internal representation of the Rust
error.

## Decision

- Public SpecGate Rust errors are opaque `ohno` errors. Callers report them
  through `Display` and inspect upstream causes through
  `std::error::Error::source`.
- Public errors do not expose `ErrorKind` enums, `kind` accessors, category
  predicates, diagnostic fields, paths, or equivalent internal state.
- Context is added at the boundary that understands the failed operation and
  retained through the `ohno` enrichment and source chain.
- A failure classification required by CTSC is a separately named protocol
  data type, not a public error-internal kind. Capture, discover, and replay
  project private failure-stage domain values for this purpose.
- Existing CTSC type names and values remain unchanged:
  `CaptureErrorKind`, `DiscoverErrorKind`, and `ReplayErrorKind` continue as
  projected wire names even though their Rust domain types are named for
  failure stages.
- Crate-private classifications may remain where implementation behavior needs
  them, including native-capture terminal-state handling. They are not part of
  the public API.
- Proc-macro parsing and expansion helpers may continue to use `syn::Result`
  inside the unpublished implementation crate because span-bearing compiler
  diagnostics are their boundary contract, not an SDK error surface.

## Consequences

The Rust API change is source-breaking for callers that matched a public kind,
called an error predicate, or read a diagnostic/path accessor. Reporting errors
and traversing their source chains continue to work. CLI syntax, output,
exit behavior, CTSC JSON shapes, and failure-stage values do not change.

Adding or changing an internal error category no longer requires a public API
change. New machine-readable classifications require an explicit domain or
protocol type rather than an error accessor.

## Validation

Compile tests and public documentation use `Display` and source chaining rather
than category accessors. Focused operation-schema tests and the generated CTSC
goldens pin the existing projected type names, fields, and enum values.
`cargo evaluate` checks the public error architecture after the migration.

## Supersedes / superseded by

This decision supersedes only the public error-taxonomy clause in
[`rust-semantic-identities-and-capture-api.md`](rust-semantic-identities-and-capture-api.md).
All identity, request, selection, and CLI compatibility decisions there remain
in force.
