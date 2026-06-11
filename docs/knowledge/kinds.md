# Operation kinds

The `kind` determines what annotations are required, what's valid, and how
extraction works.

## Kind reference

| Kind | Purpose | Required roles | Forbidden roles |
|------|---------|---------------|-----------------|
| **Stateless** | Pure input→output mapping | — | Checkpoint |
| **StateMachine** | State transitions | ≥1 Capture (before+after) | Checkpoint |
| **Sequence** | Ordered pipeline with intermediates | ≥1 Checkpoint | — |
| **ErrorMap** | Input→error classification | — | Checkpoint |
| **Structural** | Static analysis (no runtime) | — | ALL runtime roles (Setup, Mock, Capture, Checkpoint) |

## Allowed roles per kind

| Role | Stateless | StateMachine | Sequence | ErrorMap | Structural |
|------|-----------|-------------|----------|----------|-----------|
| Setup | ✓ | ✓ | ✓ | ✓ | ✗ |
| Mock | ✓ | ✓ | ✓ | ✓ | ✗ |
| Capture | ✓ | ✓ (required) | ✓ | ✓ | ✗ |
| Checkpoint (attribute) | ✗ | ✗ | ✓ (required) | ✗ | ✗ |
| Checkpoint (inline) | ✓ | ✓ | ✓ | ✓ | ✗ |

## Capture behavior by kind

| Kind | Capture timing |
|------|---------------|
| StateMachine | Before AND after the operation call (state transitions) |
| Stateless, ErrorMap, Sequence | After the operation call only (output values) |

## Extraction strategy

| Kind | What gets captured | Spec output |
|------|-------------------|-------------|
| Stateless | Captured fields / observations on return value | Test table |
| StateMachine | Captured fields before/after | State transitions |
| Sequence | Ordered checkpoint emissions | Checkpoint sequence |
| ErrorMap | Captured fields / observations, error variant | Error classification |
| Structural | Static analysis (no runtime) | Deny/must rules |

## Completeness

| Kind | Incomplete if |
|------|---------------|
| StateMachine | No Capture annotations registered |
| Sequence | No Checkpoint annotations |
| Stateless | Always complete (just needs entry point) |
| ErrorMap | Always complete (just needs entry point) |
| Structural | Always complete (just needs entry point) |

CI fails fast on incompleteness without running tests.
