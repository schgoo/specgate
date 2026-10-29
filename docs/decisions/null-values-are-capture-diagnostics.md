# Null values are capture diagnostics

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-29

## Context

CTSC has no null. Absence is representable only as the `None` of a declared
`optional<T>` ([`../ctsc/registry.md`](../ctsc/registry.md)), and linked
validation requires each recorded value, and the exact set of declared inputs,
to match the registry. C# capture can still observe a null where the declaration
is not nullable: an operation parameter or return value, a nested record member,
a property-setup argument or flattened member, or a setup-producer argument.

Earlier, capture threw `SpecGateCaptureException` (`NullValue`) out of the
woven call, so the test failed. That treated a behavioral fact about the
implementation, "this code passes null here", as a defect of the test. A null
in such a position is often evidence that the declaration should be nullable.

## Decision

A null in a non-nullable position is a diagnostic, not a rejection.

- The woven call does not throw. The test runs exactly as written.
- The C# runtime records the operation with a `specgate.unrecordable_value`
  fault completion (observer `target`). The message names the position
  (parameter, member, or return value, and the operation or setup producer).
  For a setup-producer argument, the fault is recorded on the operation the
  setup prepares.
- Capture drops that scenario (`FailedTest::Unrecordable`). The registry is not
  widened from observed data, and absence is not recorded under a non-optional
  type.
- Ordinary `specgate capture` still succeeds when other scenarios remain. It
  prints one `warning: <scenario>: <message>` line to stderr per dropped
  scenario, asking whether the position should be declared nullable (`T?`) so
  the registry declares `optional<T>`.
- Strict (golden) capture treats a dropped scenario like any other failure:
  the batch fails with the same message.

Every other capture rejection (type mismatch, ambiguous declaration, setup
missing or ambiguous, and so on) still throws.

## Consequences

- `CaptureRejection.NullValue` remains in the public enum but is reported, not
  thrown.
- `CaptureReport::warnings` carries the warning lines; the `Complete(...)`
  stdout line is unchanged.
- Rust has no null, so only C# produces this fault. CLI detection is shared.

## Validation

- `test/mechanics/csharp/SpecGate.Runtime.Tests/ValueProjectionTests.cs`:
  parameter, return value, top-level and nested property-setup nulls.
- `test/mechanics/csharp/SpecGate.Runtime.Tests/SetupMatchingTests.cs`: filled
  and setup-producer arguments.
- `rust/crates/specgate-cli/src/capture/csharp/tests.rs`:
  `an_unrecordable_value_drops_the_scenario_with_a_warning`.

## Supersedes / superseded by

Refines the null handling in
[`constructor-and-property-setups.md`](constructor-and-property-setups.md).
