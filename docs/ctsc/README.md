# Conformance Trace Semantic Conventions

The Conformance Trace Semantic Conventions (CTSC) are a vendor-neutral
interchange format for behavioral and structural conformance tools.

CTSC uses standard OpenTelemetry Protocol (OTLP) traces without adding fields to
the OTLP data model. Domain-specific semantics are expressed through fixed span
and event names plus `conformance.*` attributes.

## Conformance levels

CTSC defines three validation levels:

1. **Trace Core** validates an OTLP trace independently against CTSC trace
   conventions.
2. **Registry** validates a registry document independently against the CTSC
   registry schema and resolution rules.
3. **Linked** validates a trace against the exact registry document it
   references.

Linked validation takes:

```text
trace + root registry + required imported registries
```

It first requires valid Trace Core and Registry artifacts, verifies the
registry ID, version, and digest carried by the trace, resolves observed
operations and types, and validates trace inputs, observations, and outcomes
against the registry. Linked is a validation mode, not another CTSC document
format.

## Documents and supporting artifacts

This directory contains the **0.1 draft**:

- [`trace.md`](trace.md) — OTLP trace format and generation requirements.
- [`registry.md`](registry.md) — registry documents, operations, types, imports,
  and resolution.
- [`comparison.md`](comparison.md) — configurable comparison-policy contract
  and optional CTSC Strict reference policy.
- [`ctsc-registry-0.1.schema.json`](ctsc-registry-0.1.schema.json) — JSON Schema
  for modular registry documents.
- [`corpus/registry/`](corpus/registry/) — valid and invalid registry examples,
  including imports and dependencies.
- [`corpus/trace/`](corpus/trace/) — valid and invalid OTLP JSON and streaming
  JSONL examples.
- [`corpus/linked/`](corpus/linked/) — self-contained valid and invalid
  trace/registry pairs.

The draft is not yet a stable compatibility commitment.

## Conformance corpus

The machine-readable corpus is organized by validation level and expected
result:

```text
corpus/
├── registry/
│   ├── valid/
│   └── invalid/
├── trace/
│   ├── valid/
│   └── invalid/
└── linked/
    ├── valid/
    │   ├── registry.json
    │   ├── trace.otlp.json
    │   └── imported-type/
    │       ├── registry.json
    │       ├── tax.registry.json
    │       └── trace.otlp.json
    └── invalid/
        ├── result-type/
        │   ├── registry-expects-i64.json
        │   └── trace-returns-string.otlp.json
        └── version-mismatch/
            ├── registry.json
            └── trace.otlp.json
```

Linked corpus pairs are self-contained copies of their root registry, trace,
and any required imported registries. Valid pairs conform independently at
Trace Core and Registry levels and together at Linked level. Invalid pairs
conform independently but fail Linked validation for the documented linkage or
type mismatch.

Validate the Linked pairs directly:

```powershell
python docs\ctsc\validate.py linked `
  docs\ctsc\corpus\linked\valid\trace.otlp.json `
  docs\ctsc\corpus\linked\valid\registry.json
```

## Validate the draft artifacts

Install the validator dependencies:

```powershell
python -m pip install -r docs\ctsc\requirements.txt
```

Validate individual documents:

```powershell
python docs\ctsc\validate.py registry registry.json
python docs\ctsc\validate.py trace traces.otlp.json
python docs\ctsc\validate.py linked traces.otlp.json registry.json
```

The convenience validator checks registry schema and local references, official
OTLP JSON decoding, CTSC hierarchy and required attributes, concrete
`AnyValue` encoding, terminal outcomes, and local Linked trace-to-registry
linkage. Registry validation resolves local `file:` imports. Linked validation
of imported types, non-file retrieval, and trace comparison are not implemented.
