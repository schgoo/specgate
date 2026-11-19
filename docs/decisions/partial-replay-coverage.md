# Partial replay with explicit coverage

> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#153](https://github.com/schgoo/specgate/issues/153),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

**One missing candidate operation stops replay before any recorded test case
runs, including cases the candidate could execute.**

Why it matters: one unsupported method prevents useful checks of other methods.

- **Example:** a C# reference, the implementation being copied, records
  `get_hash_code`, `to_string`, and `to_wkt_id`. A Rust candidate, the replacement
  being tested, may implement only the last two. Replay stops at the missing
  hash operation without recording either supported result.
- **State risk:** a scenario is one recorded test case. Skipping a call inside
  that case may leave its object in the wrong state for later calls.
- **Nested calls:** replay invokes top-level calls directly. Calls recorded
  inside them are observed behavior; missing nested calls already reach
  comparison rather than blocking planning.

## Decision

**Requested:** approve explicit opt-in, whole-scenario skipping. Proposed CLI:
`replay --on-unreplayable fail|skip-scenario`, default `fail`.

| Case | Rule |
|---|---|
| Missing top-level candidate declaration | With opt-in, skip its entire scenario before any scenario operation or setup runs. |
| Supported scenario | Execute all its stimuli in original order under normal scenario-isolation rules. |
| Missing observed-only nested call | Preserve existing execution and report its observed difference. |
| Corrupt reference, ambiguous binding, build or execution failure | Do not turn the failure into a skipped scenario. |
| Other unsupported shapes | Keep existing rejection in the first slice; add recoverable categories only by explicit extension. |
| No executable scenarios | Emit a versioned coverage/no-execution finding, not a fabricated candidate trace or an equivalent verdict. |

Preflight all scenarios before execution. Extend #135's link/report schemas
with every scenario's executed/skipped state, omitted operation identities,
stable reason, reference digests, and candidate identity. Retain original
scenario identities; never edit reference artifacts or invent missing calls.

`compare` distinguishes an evidenced skip from an unexplained missing scenario.
Report all findings: an observed mismatch remains a mismatch; otherwise any
skip makes the result inconclusive. Equal executed work is not full reference
or API equivalence. No-execution findings need an explicit report-only path.

## Consequences

- Strict replay remains the default. No operation-level independence inference,
  receiver reconstruction after a skipped call, or new external-state isolation.
- Reuse #135 reports/provenance and #136 inconclusive classification; those
  issues do not already authorize operation omission. Coordinate with #137.
- Coverage is SpecGate report data, not a fake CTSC event or a relaxed Strict
  policy. Ordinary comparison without coverage keeps its existing behavior.
- Reject silent filtering and automatic hash-result adapters: both hide gaps.

## Validation

Test supported/unsupported independent scenarios, a missing call in a stateful
scenario, repeated calls, strict default, zero executable work, and missing
nested observations. Skipped scenarios must invoke neither setups nor target
code. Test digest tampering, unexplained omissions, mismatch plus skip, and
coverage round trips through `compare`. Run focused CTSC/CLI tests and
`just check`; keep complete golden coverage strict.

Implementation seams: replay request/CLI, `build_plan`, scenario execution,
versioned link/report schemas, and comparison input selection.

## Supersedes / superseded by

Extends fail-fast selection in [typed replay](typed-async-client-replay.md).
Builds on [value decomposition](value-decomposition.md#replay-inputs);
does not replace its input-default rules or implement capture profiles (#43).
