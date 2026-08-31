# Expected Linked validation failures

## Result type mismatch

[`result-type/registry-expects-i64.json`](result-type/registry-expects-i64.json) declares that
`example.answer.answer` returns `i64`.

[`result-type/trace-returns-string.otlp.json`](result-type/trace-returns-string.otlp.json) records the
result as `stringValue: "42"`.

Each artifact is valid independently. Linked validation fails because the trace
result does not conform to the registry result type.

## Registry version mismatch

[`version-mismatch/registry.json`](version-mismatch/registry.json) has registry
version `1.0.0`.

[`version-mismatch/trace.otlp.json`](version-mismatch/trace.otlp.json) references
registry version `2.0.0`.

Each artifact is valid independently. Linked validation fails because the trace
does not reference the supplied registry version.
