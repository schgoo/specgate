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
original lexical scopes. Generated, type-erased constructor and invocation
bridges expose only the selected annotated calls to a reserved libtest entry
point. Receiver producers delegate to operation-local associated bridges, keeping
receiver downcasts and method specialization arguments in the operation's scope.
Free-function generic arguments also stay in the operation's scope, preserving authored public re-exports
rather than treating compiler `type_name` output as accessible source paths.
Cargo metadata and compiler artifacts identify targets and executables,
including explicit library crate types, custom library names, and arbitrary
integration-test source paths. Test-local operations retain their compiler-reported
crate root and target even without setup or generic arguments.
No conventional `tests/` layout or test body is used as a selector.

Discovery builds the test targets, enumerates only the generated metadata
hooks, and merges test-only declarations into the normal semantic registry.
Replay compiles the selected target and runs only its generated replay hook.
Environment dependencies emitted for `SPECGATE_TEST_ENTRY` and
`SPECGATE_TEST_DISPATCH` invalidate Cargo artifacts when the generated entry or
lexical dispatch changes, without overriding candidate compiler flags or Cargo
configuration. Each scenario owns a separate scope so its receiver and setup
guards are dropped before the next scenario starts.

Structured outcomes retain the exact reference registry bytes for linked
validation before decoding and before publishing candidate output. Reachable
named declarations, including those reached only through observed nested calls,
must match; known tuple-versus-record or ownership
differences require explicit mappings, which remain unsupported. Validation
failure never replaces an existing output.

Missing or ambiguous producers and undeclared generic specializations must fail
explicitly. An annotation does not make arbitrary unannotated test environment
reproducible. Setup code must construct everything the replayed operation needs.

Rust specializations must preserve the authored return signature's syntactic
completion shape. Substitutions that change unit, optional-empty, or
success/error channels are rejected before instrumentation; for example,
`Option<T>` with `T = ()`, or `T` with `T = Result<i32, String>`. Such operations
must declare a concrete return signature. This check does not resolve aliases
or associated-type projections.

## Validation

Focused discovery, capture, planner, and generated-runner tests must exercise
semantic input renaming, receiver and parameter setup resolution, async
completion, and concrete generic typed results. Captures and replay outputs must
pass the native CTSC validators.
