# Conformance Trace Semantic Conventions 0.1

**Status:** Draft

## 1. Scope

The Conformance Trace Semantic Conventions (CTSC) define vendor-neutral
interchange between tools that record, compare, or analyze executions of
different implementations of the same behavior.

CTSC does not define a new trace envelope. A CTSC trace is standard OTLP
`TracesData`, encoded according to the OTLP protobuf or OTLP JSON mapping. A
CTSC trace file follows the OpenTelemetry Protocol File Exporter format:

- UTF-8 JSON Lines;
- one complete OTLP `TracesData` value per line;
- line separator `\n`;
- preferred extension `.jsonl`;
- no semantic dependence on file, batch, resource, scope, span, or attribute
  array ordering except where this specification explicitly says otherwise.

The words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are to
be interpreted as normative requirements.

### 1.1 Implementation independence

CTSC does not require an OpenTelemetry SDK. Producers MAY use an OpenTelemetry
SDK, official OTLP protobuf models, platform tracing APIs, or a purpose-built
recorder and encoder. Regardless of implementation, the emitted payload MUST be
valid OTLP and MUST satisfy these semantic conventions.

Using an OpenTelemetry SDK does not relax CTSC completeness requirements.
Sampling, batching, attribute limitations, or dropped telemetry that lose
comparison-relevant data invalidate the affected run.

## 2. Conformance levels

### 2.1 Trace Core

A Trace Core producer emits valid OTLP traces using the CTSC span, event,
attribute, value, and ordering conventions. A Trace Core comparator can compare
executions without a registry, using structural `AnyValue` semantics.

### 2.2 Registry

A Registry producer emits a document conforming to
[`ctsc-registry-0.1.schema.json`](ctsc-registry-0.1.schema.json). Registry
documents describe language-neutral components, operations, observations,
outcomes, and named record or tagged-union types.

### 2.3 Full

A Full producer emits Trace Core data linked to exact Registry content. A Full
comparator applies registry types recursively when interpreting trace values.

## 3. Version and namespaces

CTSC-defined attributes use the `conformance.*` namespace. Producer-specific
extensions MUST use a separate namespace, such as `example.*`.

Every CTSC resource MUST contain:

| Attribute | Type | Meaning |
|---|---|---|
| `conformance.version` | string | CTSC convention version (`0.1.0`) |
| `conformance.tool.name` | string | Producing tool |
| `conformance.tool.version` | string | Producing tool version |
| `conformance.target.name` | string | Stable target label within the run |
| `conformance.target.language` | string | Implementation language |

An OTLP `schemaUrl` SHOULD identify the same CTSC convention version. Consumers
MUST NOT assume that `schemaUrl` performs validation.

## 4. Span model

CTSC uses fixed span names. User and domain names are attributes, never span
names.

### 4.1 Span ownership and context propagation

A producer MAY emit all spans for one target execution in one process. Different
target executions do not exchange span context with one another.

When one target execution is split across processes, threads, tasks, executors,
or message handlers, context MUST be propagated across every boundary that
carries conformance work, including:

- process launch or IPC;
- async tasks and futures;
- explicitly spawned threads;
- thread pools and executors;
- message or channel handoff when the receiving work remains part of the
  scenario.

The receiving execution unit MUST start its CTSC span using the propagated
context as parent. Nested operations MUST inherit the active operation context.

Producers SHOULD use standard W3C Trace Context (`traceparent` and, when
present, `tracestate`) when context crosses a textual carrier. CTSC does not
mandate a carrier: environment variables, command arguments, headers, IPC
metadata, and in-process APIs are all permitted.

Failure to propagate context produces malformed CTSC hierarchy.

OTLP span links MAY be preserved as non-CTSC telemetry. CTSC 0.1 does not use
links to establish hierarchy, ordering, or conformance semantics.

### 4.2 Run span

```text
span.name = "conformance.run"
```

The run span is the root of one conformance execution.

One run represents one target invocation, such as one `cargo test`, `dotnet
test`, process execution, or equivalent runner call. It contains every scenario
recorded by that invocation. A trace file MAY contain one run across multiple
OTLP JSONL batches or several independent runs.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.run.id` | MUST | string unique to that target invocation |
| `conformance.run.name` | MAY | string |

### 4.3 Scenario span

```text
span.name = "conformance.scenario"
```

A scenario span MUST be a child of a run span.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.scenario.name` | MUST | string |
| `conformance.scenario.index` | SHOULD | integer |

Scenario indexes identify declaration order but do not impose execution order
between scenarios.

### 4.4 Operation span

```text
span.name = "conformance.operation"
```

An operation span MUST be a child of a scenario span, another operation span,
or an explicit parallel span.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.component.id` | MUST | string |
| `conformance.operation.name` | MUST | string |
| `conformance.operation.inputs` | MUST | `kvlistValue` |

`conformance.operation.inputs` is a map from input name to typed OTLP
`AnyValue`. Map key order is insignificant.

### 4.5 Parallel span

```text
span.name = "conformance.parallel"
```

A parallel span explicitly represents a fork/join region. It MUST be a child of
a scenario or operation span. Its direct child operation or parallel spans are
unordered relative to one another. The parallel span starts before its branches
and ends after all branches, representing the join.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.parallel.name` | MAY | string |

Producers MUST NOT use timestamp overlap alone to claim that sibling operations
are semantically parallel. Without a parallel parent, sibling operation order is
significant.

## 5. Span events

CTSC event names are fixed. Event names may repeat within a span.

### 5.1 Observation

```text
event.name = "conformance.observation"
```

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.observation.name` | MUST | string |
| `conformance.observation.value` | MUST | `AnyValue` |

### 5.2 Result

```text
event.name = "conformance.result"
```

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.result.value` | MUST | `AnyValue` |
| `conformance.result.name` | MAY | string |

An operation SHOULD emit at most one unnamed result. Tools supporting multiple
result channels MUST assign each a unique `conformance.result.name`.

### 5.3 Empty

```text
event.name = "conformance.empty"
```

This event represents successful absence from an operation that has a value
channel. It has no value attribute.

An operation MUST NOT emit both `conformance.empty` and
`conformance.result`.

Unit or void completion is different: the operation has no value channel. A
unit operation ends its span with `OK` status and emits no result or empty event.

### 5.4 Declared error

```text
event.name = "conformance.error"
```

This event represents an error declared as part of the operation contract, such
as the error branch of a result-returning operation.

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.error.name` | MUST | string |
| `conformance.error.value` | MAY | `AnyValue` |

A language implementation MAY realize a declared error using an exception,
return value, tagged union, status object, or another mechanism. The producer
MUST normalize it to `conformance.error`, not to `conformance.fault`.
The operation span MUST have `ERROR` status.

Registry error names are language-neutral and do not identify runtime exception
classes. Each producer or language binding MUST define a deterministic mapping
from implementation outcomes to declared error names.

An exception escaping user operation code but intercepted by the CTSC operation
boundary becomes `conformance.error` only when that mapping selects a declared
error. An unmapped exception becomes `conformance.fault` on the operation span.
An exception or process failure that escapes the instrumentation boundary is
reported as `conformance.fault` by the supervisor.

Different bindings may map exceptions, return statuses, or variant values to
the same declared error name. The resulting CTSC event is independent of the
implementation mechanism.

### 5.5 Fault

```text
event.name = "conformance.fault"
```

This event represents an unexpected failure. A target producer emits it on the
operation span when instrumentation catches an unexpected exception, panic, or
equivalent failure. A supervisor emits it on the nearest surviving scenario or
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

The containing span MUST have `ERROR` status. `conformance.fault.observer`
identifies the reporting boundary, such as `target` or `supervisor`.

For a supervisor-reported fault, operation attribution MUST be included only
when the supervisor knows which operation was intended. A supervisor MUST NOT
guess a nested operation in which the target may have failed. No operation span
is fabricated.

Core `conformance.fault.type` values are:

```text
launch_failure
process_exit
signal
timeout
deadlock
host_loss
export_failure
```

Target-level fault types are stable language-agnostic identifiers chosen by the
producer's native-failure mapping. Types outside the core supervisor vocabulary
MUST use a producer namespace, such as `example.arithmetic_overflow`.

Native exception or panic names MAY be preserved in
`conformance.fault.native_type`. CTSC defines no stack-trace field.

### 5.6 Operation termination

An operation that completes through its declared contract has exactly one
completion state:

```text
unit completion: no completion event
conformance.result
conformance.empty
conformance.error
```

An unexpected caught failure prevents declared completion. The operation emits
`conformance.fault` instead of a completion event.

When the target cannot report its own termination, the supervisor emits
`conformance.fault` on the nearest surviving scenario or run span. No operation
completion event is inferred.

Observations may precede any completion or failure event.

Successful unit, `conformance.result`, and `conformance.empty` operations SHOULD
have `OK` status. Declared errors and faults MUST have `ERROR` status.

### 5.7 Event order

The order of entries in a span's OTLP `events` array is their emission order and
is semantically significant. Producers and transformations claiming CTSC
compatibility MUST preserve this order. Event timestamps describe timing but do
not replace or override array order.

Intentionally unordered or parallel observations MUST be emitted in separate
child spans under `conformance.parallel`, not as competing events in one span.

## 6. Values

CTSC uses typed OTLP `AnyValue` recursively.

Every CTSC `AnyValue` MUST select one concrete OTLP value variant. An
`AnyValue` with no selected value is invalid CTSC.

| Logical value | Trace Core representation |
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
| non-string-keyed map | `arrayValue` of `{key,value}` records |
| unit | empty `kvlistValue` |
| tagged union | single-key `kvlistValue` |

### 6.1 Maps

A map whose declared key type is string uses `kvlistValue`. Each key MUST be
unique, and entry order is insignificant.

A map whose declared key type is not string uses `arrayValue`. Each array entry
MUST be a `kvlistValue` containing exactly two entries named `key` and `value`:

```text
arrayValue [
  kvlistValue {
    "key": <AnyValue>,
    "value": <AnyValue>
  }
]
```

The `key` value MUST conform to the registry map key type, and `value` MUST
conform to the registry map value type. Map keys MUST be unique under Full value
equality.

Trace Core treats a non-string-keyed map as an ordered array of entry records
because it has no registry type information. Full comparison treats it as an
unordered map. Producers SHOULD emit non-string-keyed map entries in a stable
order to improve Trace Core interoperability.

### 6.2 Integers

Registry integer primitives and their wire representations are:

| Registry type | Wire representation | Valid range |
|---|---|---|
| `i32` | `intValue` | -2,147,483,648 through 2,147,483,647 |
| `i64` | `intValue` | -9,223,372,036,854,775,808 through 9,223,372,036,854,775,807 |
| `u32` | `intValue` | 0 through 4,294,967,295 |
| `u64` | decimal `stringValue` | 0 through 18,446,744,073,709,551,615 |

A `u64` string MUST contain only ASCII decimal digits. It MUST use the shortest
representation: no leading zero is permitted unless the value is exactly
`"0"`. Signs, whitespace, separators, decimal points, and exponent notation are
invalid.

Trace Core compares `u64` encoding as a string. Full comparison uses the
registry type to parse and range-check it as an unsigned integer.

### 6.3 Floating point

Registry `f32` and `f64` values both use OTLP `doubleValue`.

An `f32` value MUST be exactly representable as IEEE 754 binary32: converting
the binary64 wire value to binary32 and back to binary64 must preserve its
value. An `f64` accepts any finite IEEE 754 binary64 value. Non-finite values
are invalid CTSC.

These rules validate the declared width. Floating-point equality and tolerance
belong to the CTSC comparison specification.

Trace Core compares the observable structure:

- arrays are ordered;
- key/value lists are unordered maps and MUST have unique keys;
- variants, records, sets, tuples, and ordinary maps are not distinguished
  beyond their structural representation.

Full comparison uses the registry type tree:

- sets are unordered and duplicate-free;
- tuples have fixed positions and arity;
- records have declared fields;
- tagged unions validate their variant tag and payload;
- integer and floating-point widths are validated.

Producers SHOULD emit sets in canonical order to improve Trace Core
interoperability. CTSC 0.1 does not define comparison for non-finite floating
point values; producers MUST NOT emit them as conformance values.

A variant with no payload MUST encode its payload as unit: an empty
`kvlistValue`. For example, `Gold` is encoded as a one-key `kvlistValue` whose
`Gold` value is an empty `kvlistValue`.

Variant names have no intrinsic CTSC meaning. A variant named `None` is an
ordinary tagged-union value and is distinct from `conformance.empty`. A source
binding maps such a variant to
`conformance.empty` only when the operation contract declares an empty outcome
and the binding defines that variant as its implementation representation.

Source bindings map implementation completion states onto the operation's
registry `outcomes` declaration. The selected completion outcome determines the
event name. Its payload, when present, is then encoded solely according to the
declared CTSC type.

Tagged unions use the same single-key `kvlistValue` representation in inputs,
observations, results, errors, record fields, collection elements, and variant
payloads. Variant names do not alter encoding or comparison rules.

Every value referenced by a Full registry MUST have a concrete CTSC type.
Third-party or unannotated source types are projected onto CTSC primitives,
records, tagged unions, tuples, collections, and maps. A registry MAY define
that projection locally or reference a named type from another component or
imported registry document.

Trace Core does not require registry types and therefore continues to compare
concrete OTLP `AnyValue` structures directly.

## 7. Registry linkage

A Full resource MUST identify one root registry document:

| Attribute | Requirement | Type |
|---|---|---|
| `conformance.registry.id` | MUST | string |
| `conformance.registry.digest` | MUST | string |
| `conformance.registry.uri` | MAY | string |

`conformance.registry.id` is the root document's stable logical identity.
`conformance.registry.digest` is `sha256:` followed by the lowercase SHA-256
digest of the root document's exact registry file bytes.
`conformance.registry.uri` is only a retrieval hint.

A registry need not be publicly available. Resolvers MAY use repository files,
adjacent bundles, authenticated servers, artifact stores, OCI registries,
in-memory data, or local caches. Network retrieval MUST be opt-in.

## 8. Reliability requirements

Conformance comparison requires complete telemetry:

- sampling MUST retain every CTSC span;
- `droppedAttributesCount`, `droppedEventsCount`, and `droppedLinksCount` MUST be
  zero on CTSC spans and events;
- rejected or partially exported spans invalidate the affected run;
- exporters MUST flush before reporting the run complete.

## 9. Comparison

Trace normalization, cross-target pairing, ordering, and equality are defined
separately in [`comparison.md`](comparison.md). Producers do not need to
implement a comparator to emit conforming CTSC artifacts.

## 10. Registry documents

A registry document is one JSON object conforming to
`ctsc-registry-0.1.schema.json` and identified by one `registryId`. A registry
file contains exactly one registry document. Registry files are ordinary JSON,
not JSONL.

A document contains one or more components and MAY import other registry
documents. It does not need to encode an entire system or dependency closure.
The root document plus the imported documents loaded for validation form the
resolved registry set.

Each operation declares its terminal outcome capabilities:

```json
{
  "outcomes": {
    "result": { "kind": "named", "name": "User" },
    "empty": true,
    "errors": [
      {
        "name": "database_error",
        "type": { "kind": "primitive", "name": "string" }
      }
    ]
  }
}
```

`result` declares the successful non-unit value type. When `result` is absent,
the operation's successful completion is unit/void and emits no completion
event. `empty: true` permits `conformance.empty` and requires a `result` value
channel. Each `errors` entry declares a permitted `conformance.error.name` and
optional payload type. Tagged unions inside any input, observation, result, or
error payload use the ordinary `tagged_union` type reference.

The core registry does not describe implementation calling conventions such as
future, task, promise, callback, or blocking invocation. Producers MAY record
those details in namespaced extensions. Concurrent execution is represented by
`conformance.parallel` spans, not by implementation-level async metadata.

The registry declares only contract-level errors through `outcomes.errors`.
Unexpected exceptions, process failures, timeouts, and other faults are
execution facts recorded in traces and are not registry declarations.

Registry references consist of:

- stable registry ID;
- exact-byte digest;
- optional location hint.

Imports MAY remain unresolved for Trace Core. Full comparison requires only the
definitions needed to interpret observed operations and values.

### 10.1 Trace operation resolution

For Full conformance, each `conformance.operation` span resolves as follows:

1. Load the root registry document identified by the resource registry ID and
   digest.
2. Resolve `conformance.component.id` to exactly one component in the resolved
   registry set.
3. Resolve `conformance.operation.name` within that component.

Component IDs MUST be unique across the resolved registry set. Operation names
MUST be unique within a component. Matching is case-sensitive.

A missing or ambiguous component or operation is invalid Full input. It is not
a behavioral mismatch.

### 10.2 Named type resolution

A named type reference resolves according to the fields it contains:

| Reference fields | Resolution scope |
|---|---|
| `name` | Current component in the current registry document |
| `componentId`, `name` | Named component in the current registry document |
| `registryId`, `componentId`, `name` | Named component in the imported registry document |

`registryId` MUST NOT appear without `componentId`. A reference containing
`registryId` resolves through an import with the same registry ID. The imported
document's exact bytes MUST match the import digest before resolving the
component and type.

Lookup does not implicitly search every imported document. A missing or
ambiguous named type is invalid Registry or Full input.

### 10.3 Imports and dependencies

An import identifies where another registry document is available:

```json
{
  "registryId": "urn:registry:tax",
  "digest": "sha256:...",
  "uri": "file:./tax.ctsc-registry.json"
}
```

A component dependency identifies which component another component may use:

```json
{
  "registryId": "urn:registry:tax",
  "componentId": "com.example.tax"
}
```

Imports locate documents. Dependencies declare component relationships. A
cross-component named type reference MUST have a matching component dependency.
For a cross-document reference, both a matching import and dependency are
required.

Unused imports MAY remain unresolved. A consumer performing Full comparison
MUST resolve every import needed by an observed operation or reachable type.

### 10.4 Name uniqueness

The following names MUST be unique in their containing scope:

- component IDs in a resolved registry set;
- operation names and named-type names in a component;
- input, observation, and declared-error names in an operation;
- field names in a record;
- variant names in a tagged union;
- import registry IDs in a registry document;
- dependency component IDs in a component.

JSON Schema validates document shape. A Registry validator enforces these
cross-item uniqueness and resolution rules.

## 11. Compatibility

Producers MUST emit only span/event names and constrained values defined by
their declared CTSC version.

Strict consumers MUST reject unknown required CTSC names or values. Lenient
consumers MAY preserve unknown CTSC data but MUST NOT reinterpret it.

Producer-specific attributes MUST NOT alter the meaning of standard CTSC data.

## 12. Security and privacy

Inputs, observations, results, faults, and registries may contain sensitive
data. CTSC does not imply that values are safe to export to an observability
backend. Producers SHOULD support local-only export, filtering, and redaction.
Comparators MUST treat redacted and unredacted values as different unless an
explicit comparison policy says otherwise.
