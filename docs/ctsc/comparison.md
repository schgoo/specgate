# CTSC Comparison 0.1

**Status:** Draft

## 1. Scope

This document defines how a CTSC comparator validates, normalizes, pairs, and
compares two or more CTSC traces. It is separate from the producer conventions:
a tool may emit conforming CTSC traces without implementing this algorithm.

CTSC 0.1 does not specify portable pairing for duplicate parallel branches.
Comparators MUST document their behavior for those traces.

## 2. Input validation

A comparator MUST:

1. decode every OTLP JSONL batch;
2. group spans by trace ID;
3. reconstruct parent/child relationships by span ID;
4. identify CTSC spans by their fixed names;
5. reject malformed CTSC hierarchy;
6. reject missing required CTSC attributes;
7. reject duplicate keys in values interpreted as maps or records;
8. reject CTSC spans or events with nonzero dropped-data counters;
9. reject an `AnyValue` with no selected value;
10. reject incomplete or partially exported runs.

Attribute array order and OTLP batch/file order are never significant.

## 3. Normalized comparison view

A comparator MUST preserve the original OTLP input and derive a normalized
semantic view for equality and diagnostics. Normalization does not delete or
rewrite the source trace.

The following data may be used to reconstruct semantics but is excluded from
equality by default:

- trace IDs and span IDs, after reconstructing relationships;
- absolute timestamp values, after deriving order;
- duration values;
- resource, scope, span, event, and link attributes outside CTSC semantics,
  unless comparison of an extension namespace was explicitly requested;
- producer-specific batching and serialization choices.

OTLP span links do not affect CTSC 0.1 comparison and are excluded from the
normalized comparison view.

Trace Core comparison uses structural `AnyValue` semantics. Full comparison
resolves registry types and applies their stronger record, tagged-union, tuple,
set, and numeric-width semantics.

For string-keyed maps, entry order is insignificant and duplicate keys are
invalid. Trace Core compares non-string-keyed map encoding as an ordered array
of entry records. Full comparison uses the registry map type, ignores entry
order, and rejects duplicate keys under Full key equality.

Full comparison validates `i32`, `i64`, and `u32` ranges on OTLP `intValue`.
Registry `u64` values are parsed from canonical decimal `stringValue`, checked
against the unsigned 64-bit range, and compared numerically. A non-canonical or
out-of-range representation is invalid Full input.

Full comparison validates that registry `f32` values round-trip exactly through
IEEE 754 binary32 and that registry `f64` values are finite binary64 values.
The equality or tolerance policy for valid floating-point values is unspecified
in CTSC Comparison 0.1.

## 4. Run and scenario pairing

The caller selects which runs to compare. When each input contains exactly one
run, selection is implicit. When an input contains multiple runs, the caller or
comparison policy must select one.

`conformance.run.id` identifies one target invocation for provenance and
diagnostics. Baseline and current runs normally have different IDs, so run IDs
are not equality or pairing keys.

Scenarios pair by `conformance.scenario.name`. Scenario names MUST be unique
within a run for portable comparison. `conformance.scenario.index` is
descriptive declaration order, not a cross-target pairing key.

Missing or additional runs or scenarios are divergences.

## 5. Event ordering

Events within one span are ordered by their position in the OTLP `events`
array. Timestamp values and timing differences are ignored for equality.
Paired events must have the same fixed event name and equivalent required CTSC
attributes and values.

## 6. Sequential child spans

Children of ordinary run, scenario, and operation spans are order-sensitive.

Non-overlapping children are ordered by their time intervals. A child that ends
before another starts precedes it.

The ordering of overlapping children outside a `conformance.parallel` span is
unspecified. Producers SHOULD wrap intentionally concurrent children in an
explicit parallel span.

Paired operation spans must have equal `conformance.component.id`,
`conformance.operation.name`, and equivalent inputs.

Sequential child spans pair by order. Repeated invocations of the same operation
therefore pair by their position among the parent's ordered child spans.

## 7. Parallel regions

Direct children of `conformance.parallel` are compared as an unordered
collection. Each branch retains its internal ordering and nested structure.

Ending the parallel span represents joining all direct branches. Work emitted
after that span is ordered after the join.

Parallel regions with unique branch identities pair branches by fixed span name
and, for operation spans, by:

```text
(conformance.component.id, conformance.operation.name)
```

Pairing duplicate parallel branches with the same identity is unspecified in
CTSC 0.1.

Unequal branch counts are divergences.

## 8. Observations and outcomes

Observations pair in event order. Paired observations must have equal
`conformance.observation.name` and equivalent values.

Results pair by event order and optional `conformance.result.name`. An operation
may have at most one unnamed result.

Unit completion pairs with unit completion. It is represented by an
operation span that ends successfully with no result, empty, error, or fault
event. In Full comparison, the registry operation has no `result` and does not
permit `empty`.

Empty outcomes pair only with empty outcomes.

Declared errors pair by `conformance.error.name` and equivalent optional error
values. In Full comparison, result, empty, and declared-error outcomes must be
permitted by the operation's registry `outcomes` declaration.

Repeated observations compare in event-array order.

## 9. Faults

Faults pair only with faults. A comparison policy determines whether
`conformance.fault.type`, message, native type, observer, and diagnostic fields
must be equivalent. CTSC defines no stack-trace comparison.

Faults are not validated against registry outcomes.

Unit, result, empty, and declared error are distinct completion outcomes.
Target and supervisor faults are failure terminations. These states are never
interchangeable.

## 10. Diagnostics

A comparator SHOULD report the first semantic divergence and its normalized
location:

- run;
- scenario;
- operation path;
- event or child-span position;
- expected and actual semantic values.

Additional divergences MAY be reported. Diagnostic formatting is not part of
CTSC compatibility.
