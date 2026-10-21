# Typed generic operations and setup-backed async replay

> **Status:** accepted
> **Owner:** @schgoo (user-ratified)
> **Date:** 2026-10-21

## Context

An annotated generic client method cannot publish an open type parameter as a
concrete CTSC type. Rust replay also rejects asynchronous operations, receivers,
setup producers, and structured results.

## Decision

- Generic operation annotations explicitly declare concrete specializations.
  Each specialization has its own semantic operation name and concrete registry
  signature. Discovery must not infer supported types from whichever tests ran.
- Replay resolves annotated Rust setup producers by semantic component,
  operation, filled input, and type mappings, using the same resolution as
  discovery. File layout, class names, and test framework conventions are not
  selectors.
- Recorded setup construction inputs are passed to the selected producers.
  Their results fill operation parameters or construct the receiver. Necessary
  environment construction, including deterministic HTTP responses, belongs
  in those existing annotated producers, not in HTTP interception.
- Replay executes recorded operations with recorded inputs. Rerunning unrelated
  candidate tests is not a substitute. Results are produced by candidate code,
  never copied from the reference recording.
- Asynchronous invocation and collections of typed results retain the existing
  concrete CTSC type and comparison rules. The CLI syntax and normative document
  formats do not change.

## Consequences

### Actual Cargo test environments

The owner additionally approved discovering and replaying inside actual Cargo
test targets. Private unit-test and integration-test producers remain in their
original lexical scopes. Generated, type-erased constructor bridges expose
only the selected annotated setup invocation to a reserved libtest entry point.
Cargo metadata and compiler artifacts identify targets and executables,
including custom library names and arbitrary integration-test source paths.
No conventional `tests/` layout or test body is used as a selector.

Discovery builds the test targets, enumerates only the generated metadata
hooks, and merges test-only declarations into the normal semantic registry.
Replay compiles the selected target and runs only its generated replay hook.
An environment dependency emitted with `option_env!("SPECGATE_TEST_ENTRY")`
invalidates Cargo artifacts when the generated entry changes, without
overriding candidate compiler flags or Cargo configuration.

Structured outcomes retain the exact reference registry bytes for linked
validation before decoding and before publishing candidate output. Reachable
named declarations must match; known tuple-versus-record or ownership
differences require explicit mappings, which remain unsupported. Validation
failure never replaces an existing output.

Missing or ambiguous producers and undeclared generic specializations must fail
explicitly. An annotation does not make arbitrary unannotated test environment
reproducible. Setup code must construct everything the replayed operation needs.

## Validation

Focused discovery, capture, planner, and generated-runner tests must exercise
semantic input renaming, receiver and parameter setup resolution, async
completion, and concrete generic typed results. Captures and replay outputs must
pass the native CTSC validators.
