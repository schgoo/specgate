# CTSC Trace 0.1

**Status:** Draft

## 1. Scope

This document defines the CTSC representation and generation requirements for
behavioral conformance traces.

The words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are
normative.

## 2. OTLP representation

A CTSC trace is standard OTLP `TracesData`, encoded using the OTLP protobuf or
OTLP JSON mapping. CTSC does not add fields to the OTLP data model.

The primary file form is one UTF-8 OTLP JSON document with the extension
`.otlp.json`. It contains one complete `TracesData` value and may be
pretty-printed.

Producers MAY instead use the OpenTelemetry Protocol File Exporter streaming
form with the extension `.otlp.jsonl`. Each JSON Lines entry contains one
complete `TracesData` batch and lines are separated by `\n`.

Container and batch boundaries have no semantic meaning.

## 3. Producer implementation

CTSC does not require an OpenTelemetry SDK. Producers MAY use an OpenTelemetry
SDK, official OTLP protobuf models, platform tracing APIs, or a purpose-built
recorder and encoder.

Regardless of implementation:

- emitted data MUST be valid OTLP;
- all comparison-relevant telemetry MUST be retained;
- implementation limitations MUST NOT change CTSC semantics.

## 4. Resource attributes

CTSC-defined attributes use the `conformance.*` namespace. Producer-specific
extensions MUST use a separate namespace.

Every resource containing CTSC spans MUST include:

| Attribute | Type | Meaning |
|---|---|---|
| `conformance.version` | string | CTSC trace version (`0.1.0`) |
| `conformance.tool.name` | string | Producing tool |
| `conformance.tool.version` | string | Producing tool version |
| `conformance.target.name` | string | Target label within the run |
| `conformance.target.language` | string | Implementation language |

An OTLP `schemaUrl` SHOULD identify the same CTSC version.

## 5. Context propagation

Different target executions do not exchange span context.

Within one target execution, context MUST be propagated across every boundary
that carries conformance work, including:

- process launch and IPC;
- async tasks and futures;
- explicitly spawned threads;
- thread pools and executors;
- message and channel handoff.

The receiving execution unit MUST start its CTSC span using the propagated
context as parent. Nested operations MUST inherit the active operation context.

Producers SHOULD use W3C Trace Context (`traceparent` and `tracestate`) when
context crosses a textual carrier. CTSC does not mandate the carrier.

Failure to propagate context produces malformed CTSC hierarchy.

OTLP span links MAY be preserved as non-CTSC telemetry. CTSC 0.1 does not use
links to establish hierarchy, ordering, or conformance semantics.

## 6. Spans

CTSC uses fixed span names. Domain names are attributes, not span names.

### 6.1 Run

```text
span.name = "conformance.run"
```

A run is the root of one target invocation, such as one `cargo test`, `dotnet
test`, process execution, or equivalent runner call. It contains every scenario
recorded by that invocation.

A streaming trace MAY contain one run across multiple batches. Either file form
MAY contain several independent runs.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.run.id` | MUST | string unique to the target invocation |
| `conformance.run.name` | MAY | string |

### 6.2 Scenario

```text
span.name = "conformance.scenario"
```

A scenario span MUST be a child of a run span.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.scenario.name` | MUST | string |
| `conformance.scenario.index` | SHOULD | integer |

Scenario indexes identify declaration order but do not impose execution order.

### 6.3 Operation

```text
span.name = "conformance.operation"
```

An operation span MUST be a child of a scenario span, another operation span,
or a parallel span.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.component.id` | MUST | string |
| `conformance.operation.name` | MUST | string |
| `conformance.operation.inputs` | MUST | `kvlistValue` |

`conformance.operation.inputs` maps input names to typed OTLP `AnyValue`
instances. Key order is insignificant.

### 6.4 Parallel region

```text
span.name = "conformance.parallel"
```

A parallel span is a fork/join region and MUST be a child of a scenario or
operation span. It starts before its direct branches and ends after all branches
complete.

Direct child operation or parallel spans are semantically unordered. Timestamp
overlap alone does not declare parallel semantics.

Outside a parallel span, sibling operation order is significant.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.parallel.name` | MAY | string |

## 7. Events

CTSC event names are fixed and may repeat within an operation span.

### 7.1 Observation

```text
event.name = "conformance.observation"
```

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.observation.name` | MUST | string |
| `conformance.observation.value` | MUST | `AnyValue` |

### 7.2 Result

```text
event.name = "conformance.result"
```

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.result.value` | MUST | `AnyValue` |

An operation MUST emit at most one result event. Additional named outputs are
observations.

### 7.3 Empty

```text
event.name = "conformance.empty"
```

Empty represents successful absence from an operation that has a value channel.
It has no value attribute.

An operation MUST NOT emit both empty and result events.

Unit or void completion is different: the operation has no value channel and
ends with `OK` status without emitting a result or empty event.

### 7.4 Declared error

```text
event.name = "conformance.error"
```

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.error.name` | MUST | string |
| `conformance.error.value` | MAY | `AnyValue` |

A declared error is part of the operation contract. Each producer or language
binding MUST define a deterministic mapping from native exceptions, return
statuses, variants, or other implementation mechanisms to registry error names.

When that mapping selects a declared error, the producer MUST emit
`conformance.error`, not `conformance.fault`. An unmapped unexpected failure
MUST emit `conformance.fault`.

The operation span MUST have `ERROR` status.

### 7.5 Fault

```text
event.name = "conformance.fault"
```

A fault is an unexpected failure.

A target producer emits it on an operation span when instrumentation catches an
unexpected failure. A supervisor emits it on the nearest surviving scenario or
run span when the target cannot report its own termination.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.fault.type` | MUST | string |
| `conformance.fault.message` | MAY | string |
| `conformance.fault.native_type` | MAY | string |
| `conformance.fault.observer` | MUST | string |
| `conformance.fault.phase` | MAY | string |
| `conformance.fault.operation.name` | MAY | string |
| `conformance.fault.operation.component_id` | MAY | string |
| `conformance.fault.exit_code` | MAY | integer |
| `conformance.fault.signal` | MAY | string |
| `conformance.fault.timeout_ms` | MAY | integer |

The containing span MUST have `ERROR` status.
`conformance.fault.observer` identifies the reporting boundary, such as
`target` or `supervisor`.

For supervisor faults, operation attribution MUST be included only when the
supervisor knows which operation was intended. A supervisor MUST NOT guess a
nested operation. No operation span is fabricated.

Core supervisor fault types are:

```text
launch_failure
process_exit
signal
timeout
deadlock
host_loss
export_failure
```

Target-level fault types are stable language-neutral identifiers chosen by the
producer's native-failure mapping. Other fault types MUST use a producer
namespace, such as `example.arithmetic_overflow`.

Native failure names MAY be preserved in
`conformance.fault.native_type`. CTSC defines no stack-trace field.

### 7.6 Operation termination

An operation completing through its contract has exactly one completion state:

```text
unit completion: no completion event
conformance.result
conformance.empty
conformance.error
```

The producer maps the implementation completion state onto the operation's
registry `outcomes` declaration. The selected outcome determines the completion
event. Value variant names do not determine completion semantics.

An unexpected caught failure emits `conformance.fault` instead of a completion
event. When the target cannot report termination, the supervisor emits a fault
on the nearest surviving scenario or run span.

Observations may precede completion or failure.

Successful unit, result, and empty operations SHOULD have `OK` status. Declared
errors and faults MUST have `ERROR` status.

### 7.7 Event order

Entries in a span's OTLP `events` array are in emission order. Producers and
transformations claiming CTSC compatibility MUST preserve that order.

Event timestamps describe timing but do not replace or override array order.
Intentionally unordered or parallel observations MUST use separate child spans
under `conformance.parallel`.

## 8. Values

Every CTSC `AnyValue` MUST select one concrete OTLP value variant. An unset
`AnyValue` is invalid CTSC.

| Logical value | OTLP representation |
|---|---|
| string | `stringValue` |
| boolean | `boolValue` |
| signed 32/64-bit integer | `intValue` |
| unsigned 32-bit integer | non-negative `intValue` |
| unsigned 64-bit integer | canonical decimal `stringValue` |
| floating point | `doubleValue` |
| bytes | `bytesValue` |
| ordered list or tuple | `arrayValue` |
| set | `arrayValue` |
| string-keyed map or record | `kvlistValue` |
| non-string-keyed map | `arrayValue` of entry records |
| unit | empty `kvlistValue` |
| tagged union | single-key `kvlistValue` |

### 8.1 Maps

A string-keyed map uses `kvlistValue`; keys MUST be unique and order is
insignificant.

A non-string-keyed map uses `arrayValue`. Each entry MUST be a `kvlistValue`
containing exactly `key` and `value`:

```text
arrayValue [
  kvlistValue {
    "key": <AnyValue>,
    "value": <AnyValue>
  }
]
```

Under Linked validation, keys and values MUST conform to the registry map types
and keys MUST be unique. Trace Core treats this encoding as an ordered array.
Producers SHOULD emit non-string-keyed map entries in a stable order to improve
Trace Core interoperability.

### 8.2 Integers

| Registry type | Wire representation | Valid range |
|---|---|---|
| `i32` | `intValue` | -2,147,483,648 through 2,147,483,647 |
| `i64` | `intValue` | -9,223,372,036,854,775,808 through 9,223,372,036,854,775,807 |
| `u32` | `intValue` | 0 through 4,294,967,295 |
| `u64` | decimal `stringValue` | 0 through 18,446,744,073,709,551,615 |

A `u64` string contains only ASCII decimal digits and uses the shortest
representation. Leading zeroes are prohibited except for `"0"`. Signs,
whitespace, separators, decimal points, and exponent notation are invalid.

### 8.3 Floating point

Registry `f32` and `f64` values use `doubleValue`.

An `f32` value MUST be exactly representable as IEEE 754 binary32. An `f64`
accepts any finite IEEE 754 binary64 value. Non-finite values are invalid CTSC.

Floating-point equality and tolerance belong to comparison policy.

### 8.4 Unit and tagged unions

Unit uses an empty `kvlistValue`.

A tagged union uses a single-key `kvlistValue`. Its key is the variant name and
its value is the variant payload. A variant without payload uses unit.

Variant names do not alter encoding or operation-completion rules. A tagged
union remains a value unless the producer's operation-outcome mapping selects a
completion state before encoding the payload.

Producers SHOULD emit sets in a stable order to improve Trace Core
interoperability.

### 8.5 Linked type validation

Every value validated in Linked mode has a concrete CTSC registry type.
Third-party or unannotated source values are projected onto CTSC primitives,
records, tagged unions, tuples, collections, and maps.

Trace Core compares concrete `AnyValue` structure without registry types.

## 9. Registry linkage

A resource used for Linked validation MUST identify one root registry document:

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.registry.id` | MUST | string |
| `conformance.registry.version` | MUST | string |
| `conformance.registry.digest` | MUST | string |
| `conformance.registry.uri` | MAY | string |

The ID identifies the root registry family. The version identifies the source
package, release, commit, or build context. The digest is `sha256:` followed by
the lowercase SHA-256 digest of the exact generated registry file bytes. The URI
is only a retrieval hint.

Resolvers MAY use local files, adjacent bundles, authenticated servers,
artifact stores, OCI registries, memory, or local caches. Network retrieval
MUST be opt-in.

## 10. Completeness

Conformance traces require complete telemetry:

- sampling MUST retain every CTSC span;
- dropped attribute, event, and link counts MUST be zero;
- rejected or partially exported spans invalidate the affected run;
- exporters MUST flush before reporting the run complete.

## 11. Compatibility and privacy

Producers MUST emit only names and constrained values defined by their declared
CTSC version.

Producer-specific attributes MUST NOT alter standard CTSC meaning.

Trace inputs, observations, results, errors, and faults may contain sensitive
data. CTSC does not imply that values are safe to export to an observability
backend. Producers SHOULD support local-only export, filtering, and redaction.
