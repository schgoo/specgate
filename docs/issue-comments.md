# Issue Comments for specgate

## Issue #1 — Async/task-local trace collection

### Proposed solution

Add `async: true` on operations in the spec. The harness validates this against annotations — if the spec says async but the annotation is `fn` (not `async fn`), that's a validation error, and vice versa.

**Runtime:** The generated runner uses `#[tokio::main(flavor = "current_thread")]` initially. This preserves thread-local trace collection while supporting `.await`. For the OData library use case (sequential async I/O, not concurrent spawning), current_thread is sufficient.

**Future:** If the implementation uses `tokio::spawn` or a multi-threaded runtime (e.g., via anyspawn), we'll need task-local or context-passing trace collection. Deferring that until we have a concrete use case that breaks current_thread.

**Span IDs:** Each operation invocation gets a unique span ID on its trace events. This enables correlation when multiple async operations interleave. The harness groups events by span before matching against expectations.

**Schema change:**
```yaml
operations:
  get_entity:
    async: true
    inputs: { id: string }
    outputs: [get_entity.outcome, get_entity.result]
```

---

## Issue #2 — Concurrent trace isolation (DEFERRED)

### Analysis

The interleaving problem described here is real, but it's addressed by **span IDs** (see #1) rather than scoped buffers. Each operation invocation gets a unique span, and all its events carry that span ID. The harness groups events by span before matching.

For the OData library use case, concurrent execution isn't a concern — the library performs sequential async I/O, not concurrent spawning. Batch processing constructs/parses batch payloads; it doesn't execute sub-requests concurrently.

**Deferring** until we have a concrete use case (e.g., server-side OData with concurrent request handlers) that requires scoped buffers beyond what spans provide.

If needed later, the `concurrent:` step keyword (see #5 comment) provides the spec-level abstraction, and the runtime would need scoped or task-local buffers to isolate events per span.

---

## Issue #3 — Spec provenance metadata

### Proposed solution

Add `source:` field on cases and `level:` as a peer field:

```yaml
cases:
  - name: duplicate_property_rejected
    level: must
    source:
      assertion_ids: [CSDL-XML-6-A5]
      spec: "CSDL XML v4.01"
      section: "§6"
    operation: add_property
    inputs: { ... }
    expected: [ ... ]
```

- `source:` is opaque to the harness — it passes through to reporting/coverage tools.
- `assertion_ids` is a list (many-to-many: one case can cover multiple assertions, multiple cases can cover one assertion).
- `level` stays at the case level, not nested inside `source` — they're independent concepts (level is harness-actionable, source is metadata).
- No `tags` field for now — can add later if needed.

Reporting tools can compute: "of 3,219 OData assertions, N have at least one test case."

---

## Issue #4 — Normative strength (MUST/SHOULD/MAY)

### Proposed solution

Add `level: must | should | may` on cases. Default is `must`.

The level affects **only what happens when the annotation is missing** (operation not implemented):

| Level | Annotation missing | Annotation present, case fails | Annotation present, case passes |
|-------|-------------------|-------------------------------|-------------------------------|
| must  | ERROR             | FAIL                          | PASS                          |
| should| WARN              | FAIL                          | PASS                          |
| may   | SKIP              | FAIL                          | PASS                          |

**Key rule:** If you implement it, it must work correctly. The level is about "is this operation required to exist?" — once it exists, pass/fail is absolute.

This keeps the harness simple — it's a reporting concern, not a matching concern. The matcher doesn't care about level; it only affects the summary report.

---

## Issue #5 — Unordered/set-based matching

### Proposed solution

Add `$`-prefixed directives in the `expected` list to distinguish keywords from event names:

- `$run: <name>` — matches a Run event (replaces bare `run:`)
- `$unordered: [...]` — items must all appear between surrounding ordered assertions, any order
- `$anywhere: [...]` — items must appear somewhere in the entire trace, position irrelevant
- Bare `name: value` — ordered Event assertion (unchanged)

```yaml
expected:
  - $run: process_batch
  - batch.count: "3"
  - $unordered:
    - op1.status: "200"
    - op2.status: "201"
  - batch.complete: "ok"
  - $anywhere:
    - log.level: "info"
```

Multiple of any directive allowed. The `$` prefix prevents collision with event names — an event literally named "run" or "unordered" works fine without the prefix.

**Steps also gain `concurrent:` blocks** (no `$` prefix needed since steps don't have event names):

```yaml
steps:
  - operation: init_batch
  - concurrent:
    - operation: get_entity
      inputs: { id: "1" }
    - operation: get_entity
      inputs: { id: "2" }
  - operation: finalize
```

`concurrent:` acts as a barrier — fan out, join, then continue to next sequential step. Each concurrent operation gets its own span for trace correlation.

A test case will be added for event names that match keywords (e.g., an operation named "run") to prove the `$` prefix prevents collision.
