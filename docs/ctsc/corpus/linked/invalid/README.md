# Expected Linked validation failures

## Result type mismatch

[`result-type/registry-expects-i64.json`](result-type/registry-expects-i64.json) declares that
`example.answer.answer` returns `i64`.

[`result-type/trace-returns-string.otlp.json`](result-type/trace-returns-string.otlp.json) records the
result as `stringValue: "42"`.

Each artifact is valid independently. Linked validation fails because the trace
result does not conform to the registry result type.

## Optional collapsed to its payload

[`optional-encoding/registry-expects-optional.json`](optional-encoding/registry-expects-optional.json)
declares that `example.lookup.find` returns `optional<string>`.

[`optional-encoding/trace-returns-bare-string.otlp.json`](optional-encoding/trace-returns-bare-string.otlp.json)
records the result as a bare `stringValue: "alice"`.

Each artifact is valid independently. Linked validation fails because an
`optional<T>` must always be encoded as its `Some` or `None` variant; a present
value may never be written as the bare payload, since that would make presence
indistinguishable from a non-optional `string`.

## Registry version mismatch

[`version-mismatch/registry.json`](version-mismatch/registry.json) has registry
version `1.0.0`.

[`version-mismatch/trace.otlp.json`](version-mismatch/trace.otlp.json) references
registry version `2.0.0`.

Each artifact is valid independently. Linked validation fails because the trace
does not reference the supplied registry version.
