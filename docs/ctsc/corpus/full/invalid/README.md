# Expected Full validation failure

[`registry-expects-i64.json`](registry-expects-i64.json) declares that
`example.answer.answer` returns `i64`.

[`trace-returns-string.otlp.jsonl`](trace-returns-string.otlp.jsonl) records the
result as `stringValue: "42"`.

Each artifact is valid independently. Full validation fails because the trace
result does not conform to the registry result type.
