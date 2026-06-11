# Operation kinds

The `kind` determines what annotations are required, what's valid, and how
extraction works.

## Kind reference

| Kind | Purpose | Required roles | Forbidden roles |
|------|---------|---------------|-----------------|
| **Stateless** | Pure input→output mapping | — | State, Checkpoint |
| **StateMachine** | State transitions | ≥1 State | Checkpoint |
| **Sequence** | Ordered pipeline with intermediates | ≥1 Checkpoint | State |
| **ErrorMap** | Input→error classification | — | State, Checkpoint |
| **Structural** | Static analysis (no runtime) | — | ALL runtime roles (Setup, Mock, State, Checkpoint) |

## Allowed roles per kind

| Role | Stateless | StateMachine | Sequence | ErrorMap | Structural |
|------|-----------|-------------|----------|----------|-----------|
| Setup | ✓ | ✓ | ✓ | ✓ | ✗ |
| Mock | ✓ | ✓ | ✓ | ✓ | ✗ |
| Checkpoint | ✗ | ✗ | ✓ (required) | ✗ | ✗ |
| State | ✗ | ✓ (required) | ✗ | ✗ | ✗ |

## Extraction strategy

| Kind | What gets captured | Spec output |
|------|-------------------|-------------|
| Stateless | args, return_value | Test table |
| StateMachine | State before/after | State transitions |
| Sequence | Ordered emissions | Checkpoint sequence |
| ErrorMap | args, error_variant | Error classification |
| Structural | Static analysis (no runtime) | Deny/must rules |

## Completeness

| Kind | Incomplete if |
|------|---------------|
| StateMachine | No State annotations registered |
| Sequence | No Checkpoint annotations |
| Stateless | Always complete (just needs entry point) |
| ErrorMap | Always complete (just needs entry point) |
| Structural | Always complete (just needs entry point) |

CI fails fast on incompleteness without running tests.
