# CTSC Registry 0.1

**Status:** Draft

## 1. Scope

This document defines the CTSC language-neutral structural registry.

Registry documents conform to
[`ctsc-registry-0.1.schema.json`](ctsc-registry-0.1.schema.json).

The words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are
normative.

## 2. Documents

A registry document is one JSON object with:

```json
{
  "format": "ctsc.registry",
  "formatVersion": "0.1.0",
  "registryId": "...",
  "version": "...",
  "components": []
}
```

`format` and `formatVersion` identify the registry schema. `registryId`
identifies the registry family. `version` identifies the source package,
release, commit, or build context from which the document was generated. A
registry file contains exactly one registry document and is ordinary JSON, not
JSONL.

The same `registryId` and `version` MAY be regenerated during development. The
document digest distinguishes exact generated snapshots. The tuple
`(registryId, version, digest)` identifies exact registry content.

A document contains one or more components and MAY import other registry
documents. It does not need to encode an entire system or dependency closure.

The root document plus imported documents loaded for validation form the
resolved registry set.

## 3. Components

A component is identified by `id` and contains:

- operation declarations;
- named type declarations;
- component dependencies;
- optional namespaced extensions.

Component IDs MUST be unique across a resolved registry set.

## 4. Operations

An operation contains:

- unique operation name;
- ordered input declarations;
- observation declarations;
- completion outcomes;
- optional description and extensions.

The core registry does not describe implementation calling conventions such as
future, task, promise, callback, or blocking invocation. Producers MAY record
those details in namespaced extensions.

Concurrent execution is represented by CTSC parallel spans, not async metadata.

### 4.1 Outcomes

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

`result` declares a successful non-unit value type.

When `result` is absent, successful completion is unit/void and emits no
completion event.

`empty: true` permits `conformance.empty` and requires `result`.

Each error entry declares a permitted `conformance.error.name` and optional
payload type.

Unexpected exceptions, process failures, timeouts, and other faults are trace
facts and are not registry declarations.

## 5. Type system

Every value interpreted through Linked validation has a concrete CTSC type.

### 5.1 Primitive types

```text
unit
string
bool
i32
i64
u32
u64
f32
f64
bytes
```

### 5.2 Named type

```json
{
  "kind": "named",
  "name": "OrderLine"
}
```

Named references may additionally specify `componentId` and `registryId`
according to Section 8.

### 5.3 Lists and sets

```json
{
  "kind": "list",
  "items": { "kind": "primitive", "name": "string" }
}
```

```json
{
  "kind": "set",
  "items": { "kind": "primitive", "name": "string" }
}
```

### 5.4 Maps

```json
{
  "kind": "map",
  "keys": { "kind": "primitive", "name": "string" },
  "values": { "kind": "primitive", "name": "i64" }
}
```

### 5.5 Tuple

```json
{
  "kind": "tuple",
  "items": [
    { "kind": "primitive", "name": "i32" },
    { "kind": "primitive", "name": "string" }
  ]
}
```

### 5.6 Record

Records may be named component types or inline type references:

```json
{
  "kind": "record",
  "fields": [
    {
      "name": "id",
      "type": { "kind": "primitive", "name": "i64" }
    }
  ]
}
```

### 5.7 Tagged union

Tagged unions may be named component types or inline type references:

```json
{
  "kind": "tagged_union",
  "variants": [
    { "name": "Empty" },
    {
      "name": "Value",
      "payload": { "kind": "primitive", "name": "string" }
    }
  ]
}
```

Variant names have no intrinsic CTSC meaning.

### 5.8 Third-party types

Third-party or unannotated source types are projected onto CTSC primitives,
records, tagged unions, tuples, collections, and maps.

The projection MAY be declared locally or reference a named type from another
component or imported registry document.

## 6. Imports

An import identifies another registry document:

```json
{
  "registryId": "urn:registry:tax",
  "version": "2.1.0",
  "digest": "sha256:...",
  "uri": "file:./tax.ctsc-registry.json"
}
```

The digest is `sha256:` followed by the lowercase SHA-256 digest of the exact
imported registry file bytes. The URI is an optional retrieval hint.

Unused imports MAY remain unresolved. A Linked validator MUST resolve every import
needed by an observed operation or reachable type.

Network retrieval MUST be opt-in.

## 7. Component dependencies

A dependency identifies a component used by another component:

```json
{
  "registryId": "urn:registry:tax",
  "componentId": "com.example.tax"
}
```

Imports locate documents. Dependencies declare component relationships.

A cross-component named type reference MUST have a matching dependency. A
cross-document reference requires both a matching import and dependency.

## 8. Named type resolution

A named reference resolves according to its fields:

| Reference fields | Resolution scope |
|---|---|
| `name` | Current component in the current registry document |
| `componentId`, `name` | Named component in the current registry document |
| `registryId`, `componentId`, `name` | Named component in the imported registry document |

`registryId` MUST NOT appear without `componentId`.

Lookup does not implicitly search imported documents.

For imported references:

1. Resolve an import with the same registry ID and version.
2. Verify the imported document digest.
3. Resolve the component ID.
4. Resolve the named type.

A missing or ambiguous type is invalid Registry or Linked input.

## 9. Trace operation resolution

For Linked validation, each trace operation resolves as follows:

1. Load the root registry identified by the trace resource ID and digest.
2. Resolve `conformance.component.id` to exactly one component in the resolved
   registry set.
3. Resolve `conformance.operation.name` within that component.

A missing or ambiguous component or operation is invalid Linked input, not a
behavioral mismatch.

## 10. Name uniqueness

The following names MUST be unique in their containing scope:

- component IDs in a resolved registry set;
- operation names and named-type names in a component;
- input, observation, and declared-error names in an operation;
- field names in a record;
- variant names in a tagged union;
- import registry IDs in a registry document;
- dependency component IDs in a component.

JSON Schema validates document shape. Registry validation enforces cross-item
uniqueness and resolution.

## 11. Compatibility and privacy

Extensions MUST use a namespace outside `conformance.*`.

Registry documents may reveal internal API and data structure information.
Resolvers SHOULD support local files, authenticated artifact stores, and
offline operation.
