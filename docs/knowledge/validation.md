# Validation rules

`core.validate` checks that extracted annotations form structurally correct
operations. It produces a `Valid` or `Invalid` outcome, plus warnings.

## Errors (make outcome Invalid)

| Error | Condition |
|-------|-----------|
| `MissingEntryPoint` | Annotations reference an operation with no `SpecOperation` |
| `DuplicateEntryPoint` | Two `SpecOperation` annotations for the same operation name |
| `IncompleteOperation` | StateMachine has no Capture, or Sequence has no Checkpoint |
| `InvalidRoleForKind` | Role not allowed for this kind (see kinds.md) |
| `ConflictingParamNames` | Two setups contribute the same parameter name |
| `OrphanAnnotation` | Annotation references operation name not found (likely typo) |
| `DuplicateMockName` | Two mocks on the same operation with the same `mock_name` |
| `DuplicateSetupName` | Two setups on the same operation with the same `name` |

## Warnings (outcome can still be Valid)

| Warning | Condition |
|---------|-----------|
| `AbstractTypeNoGenerator` | Abstract type in parameters without a generator |
| `PrivateCheckpoint` | Checkpoint has private accessibility — harness may not reach it |
| `AmbiguousConstruction` | Entry point is a method but no `spec_setup` exists for its type |

## Key rules

- Errors and warnings are reported independently — an Invalid result can still have warnings
- Validation is per-operation: one invalid operation makes the whole result Invalid
- Duplicate mock/setup names are scoped per operation — same name in different operations is fine
- Empty annotation list is vacuously Valid
- Structural kind forbids ALL runtime roles (Setup, Mock, Capture, Checkpoint)
- Setup with `self` in params is an error (setups must be free functions)
