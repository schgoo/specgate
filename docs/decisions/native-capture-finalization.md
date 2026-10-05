# Native capture finalization

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-01

## Context

Native operation and setup guards previously performed fallible work from
`Drop`. In particular, completing an environment-driven top-level operation
could persist its sidecar during destruction, and an I/O failure could panic
while another panic was already unwinding. Suppressing that failure would make
capture appear successful without durable output, while finalizing the whole
session after one operation would lose later top-level operations in the same
libtest scenario.

## Decision

- Annotation-generated synchronous operation completion is explicit and
  consuming. It closes the operation in memory before performing any required
  sidecar persistence and returns persistence failures.
- Generated operation code catches a target unwind, consumes the scope through
  explicit fault completion, and resumes the original panic payload. This
  preserves caught-fault artifacts and target panic propagation without
  destructor persistence.
- `OperationScope::drop` performs only infallible, in-memory cleanup. During
  target unwind it records the existing unexpected-target fault when possible;
  it never performs I/O, panics, or writes diagnostics.
- Deferred setup inputs have an explicit consuming `commit`. Dropping
  uncommitted setup inputs has no fallible behavior.
- Environment-driven capture persists the complete capture-so-far after every
  successfully completed top-level operation. This is cumulative snapshotting,
  not whole-session finalization. `finish_native_capture` remains the explicit
  endpoint for manually started in-process sessions.
- Runtime persistence uses a concrete `FileSystem` with `Real` and test-only
  `Fake` enum variants. No public extension trait is introduced. The fake can
  deterministically fail directory creation, temporary-file creation, write,
  flush, sync, replacement, and replacement retries.
- A persistence failure terminalizes the active capture session. In an isolated
  environment-driven capture child, generated instrumentation writes the
  diagnostic with one hidden stable marker from the runtime facade while
  preserving the target return. The parent CLI inspects captured output even
  after a successful libtest exit and reports `CaptureErrorKind::Execution`
  instead of treating the process as successful or as an ordinary failed test.
  Ordinary failed tests remain skippable in product capture and rejected by
  strict golden capture.

## Consequences

Normal-completion persistence failures are visible to users and cannot trigger
a second persistence attempt from guard destruction. There is deliberately no
crash-durable or partial-output guarantee: fault persistence occurs only while
generated instrumentation can explicitly finalize the boundary, never from
destructor I/O.

This decision does not change CLI arguments, CTSC artifact shapes, fault
vocabulary, filtering, stimulus selection, or setup folding. It adds no custom
harness, process-exit hook, public CLI option, task-safe context, or general
runtime module split. Async/task-safe capture remains separate M1 work.

## Validation

Runtime tests cover successful and injected persistence stages, replacement
retries, terminal sessions, no second destructor write or panic, caught unwind
faults, and cumulative multi-operation snapshots. Facade tests cover generated
completion, nesting, early return, `?`, target panic propagation, setup unwind,
stacked setups, and explicit setup commit. CLI tests cover marker promotion,
ordinary failed-test behavior, linked bundle validation, and unchanged artifact
encoding. The CTSC smoke and golden gates pin artifact compatibility.

## Supersedes / superseded by

None.
