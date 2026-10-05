# Rust semantic identities and capture API

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-01

## Context

Rust public APIs represented component, operation, and target identities as
undifferentiated strings. The capture library API also accepted four positional
string arguments, including filesystem paths, and represented failure as a
success-shaped `CaptureOutcome::Error` variant. This made identity intent,
future-compatible validation, path handling, and programmatic failure handling
unclear.

The existing empty target and component strings are selection sentinels. Any
foundation types must preserve every string currently accepted, including those
empty values, and must remain transparent CTSC strings.

## Decision

- Define `ComponentId`, `OperationName`, and `TargetName` in
  `specgate-runtime` and re-export them through the `specgate` facade.
- Accept all strings without lexical validation. Constructors and public API
  signatures use the semantic types now so future lexical policy can be added
  without another signature-shape migration.
- Serialize and project each identity as its contained CTSC string, never as a
  wrapper object.
- Represent public capture inputs with one owned `CaptureRequest`. Its
  `CaptureRequestBuilder` accepts required filesystem dependencies through
  named `CapturePaths` fields; target and component selection use the semantic
  types.
- Keep the annotated operation name `capture`, with the accepted structured
  request input shape.
- Return `Result<CaptureReport, CaptureError>`. `CaptureError` uses ohno and is
  available at the crate root. Its stable stage taxonomy is available as
  `specgate_cli::capture_error::CaptureErrorKind`; callers normally use focused
  predicates on the error.
- Preserve capture selection semantics and the public CLI command, arguments,
  defaults, usage, formatting, and exit behavior.

## Consequences

The Rust library capture API is intentionally source-breaking. The CLI is not.
Empty target and component identities continue to select defaults exactly as
before. No lexical rule is established by this decision, and no normative CTSC
registry or trace shape changes: identity and projected path values remain CTSC
strings.

Golden-only batch exclusions remain a separate crate-private request so product
capture inputs are not coupled to matrix policy. Runtime finalization, floating
point behavior, capture context propagation, replay semantics, and broad error
migration remain deferred.

## Validation

Focused runtime tests pin unrestricted construction, serde transparency, native
CTSC string projection, and string-shaped operation sidecars. Capture tests pin
the structured request projection, typed request failure, and existing linked
bundle artifacts. CLI parser tests pin `PathBuf` storage without changing syntax.
The existing golden matrix continues to validate capture and replay semantics.

## Supersedes / superseded by

None.
