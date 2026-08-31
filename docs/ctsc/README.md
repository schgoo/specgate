# Conformance Trace Semantic Conventions

The Conformance Trace Semantic Conventions (CTSC) are a vendor-neutral
interchange format for behavioral and structural conformance tools.

CTSC uses standard OpenTelemetry Protocol (OTLP) traces without adding fields to
the OTLP data model. Domain-specific semantics are expressed through fixed span
and event names plus `conformance.*` attributes.

This directory contains the **0.1 draft**:

- [`specification.md`](specification.md) — trace, registry, normalization, and
  producer requirements.
- [`comparison.md`](comparison.md) — normalization, pairing, ordering, and
  comparison requirements.
- [`ctsc-registry-0.1.schema.json`](ctsc-registry-0.1.schema.json) — JSON Schema
  for modular registry documents.
- [`corpus/registry/valid/order-pricing.registry.json`](corpus/registry/valid/order-pricing.registry.json)
  — registry example.
- [`corpus/trace/valid/sequential.otlp.json`](corpus/trace/valid/sequential.otlp.json) — sequential
  run/scenario/operation example.
- [`corpus/trace/valid/parallel.otlp.json`](corpus/trace/valid/parallel.otlp.json) — explicit
  parallel-region example.
- [`corpus/trace/valid/supervisor-fault.otlp.json`](corpus/trace/valid/supervisor-fault.otlp.json)
  — target-process failure recorded by a surviving supervisor.
- [`corpus/trace/valid/unit.otlp.json`](corpus/trace/valid/unit.otlp.json) — successful operation
  completion without a value channel.
- [`corpus/trace/valid/outcomes.otlp.json`](corpus/trace/valid/outcomes.otlp.json) — empty and
  declared-error operation outcomes.

CTSC defines three conformance levels:

1. **Trace Core** — standalone behavioral OTLP traces.
2. **Registry** — standalone structural registry documents.
3. **Full** — traces linked to exact registry content.

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
    ├── valid/
    └── invalid/
└── full/
    ├── valid/
    │   ├── registry.json
    │   └── trace.otlp.json
    └── invalid/
        ├── registry-expects-i64.json
        └── trace-returns-string.otlp.json
```

The Full corpus pairs are self-contained copies of a registry and trace. The
valid pair conforms at both Trace Core and Registry levels. The invalid pair is
individually valid at those levels but fails Full validation because its result
value does not match the registry type.

Validate the Full pairs directly:

```powershell
python docs\ctsc\validate.py full `
  docs\ctsc\corpus\full\valid\trace.otlp.json `
  docs\ctsc\corpus\full\valid\registry.json
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
python docs\ctsc\validate.py full traces.otlp.json registry.json
```

The convenience validator checks registry schema and local references, official
OTLP JSON decoding, CTSC hierarchy and required attributes, concrete
`AnyValue` encoding, terminal outcomes, and local Full trace-to-registry
linkage. Imported-registry retrieval and trace comparison are not implemented.
