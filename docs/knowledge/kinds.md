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

## Test generation for StateMachine specs

State machine specs have `state`, `init`, `operations`, and cases with `steps`.
Each test follows the component lifecycle:

```
1. Create component instance (SpecSetup)
2. Verify initial state matches `init`
3. For each step:
   a. Call the operation with inputs
   b. If step has `expected` — assert return value (partial match)
   c. If step has `assert_state` — read state via SpecCapture, assert (partial match)
```

### Test structure (Rust example)

```rust
#[test]
fn add_shapes_updates_area() {
    // Setup
    let mut canvas = make_canvas(800.0, 600.0);

    // Verify init state
    assert_eq!(canvas.total_area, 0.0);

    // Step 1: add a circle
    canvas.add_shape(Shape::Circle { radius: 5.0 });
    assert!((canvas.total_area - 78.54).abs() < 0.01);

    // Step 2: add a rectangle
    canvas.add_shape(Shape::Rectangle { width: 3.0, height: 4.0 });
    assert!((canvas.total_area - 90.54).abs() < 0.01);
}
```

### Single-step state machine cases

When a state machine case has only one step, the test still follows the lifecycle
(create, verify init, call operation, assert). It just has one step instead of
many. This differs from a Stateless test only in that state is verified.

## Completeness

| Kind | Incomplete if |
|------|---------------|
| StateMachine | No Capture annotations registered |
| Sequence | No Checkpoint annotations |
| Stateless | Always complete (just needs entry point) |
| ErrorMap | Always complete (just needs entry point) |
| Structural | Always complete (just needs entry point) |

CI fails fast on incompleteness without running tests.
