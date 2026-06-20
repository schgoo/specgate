# Fixture Spec Index

Canonical examples of every SpecGate feature, organized by topic.
All files live under `test/rust/crates/specgate-fixtures/specs/`.

## Basics

| Feature | Fixture |
|---------|---------|
| Simplest possible spec | `stateless_add.spec.yaml` |
| Multiple cases in one spec | `multi_case.spec.yaml` |
| Void operation (no return) | `void_operation.spec.yaml` |
| Read-only operation | `readonly_operation.spec.yaml` |

## State & Setup

| Feature | Fixture |
|---------|---------|
| Setup + stateful operation | `statemachine_counter.spec.yaml` |
| Setup with input parameters | `setup_with_params.spec.yaml` |
| Multiple setups (multi-alias) | `multi_setup.spec.yaml` |
| Multi-step case (sequential ops) | `multi_step.spec.yaml` |
| Multiple field mutations | `multi_mutation.spec.yaml` |
| Nested operations | `nested_operations.spec.yaml` |

## Return Types

| Feature | Fixture |
|---------|---------|
| Result — Ok path | `result_ok.spec.yaml` |
| Result — Error path | `result_err.spec.yaml` |
| Option — Some path | `option_some.spec.yaml` |
| Option — None path | `option_none.spec.yaml` |
| Panic / Unrecoverable | `unrecoverable.spec.yaml` |

## Trace Matching

| Feature | Fixture |
|---------|---------|
| Subsequence with gaps | `subsequence_with_gaps.spec.yaml` |
| Wrong order fails | `subsequence_wrong_order.spec.yaml` |
| `$unordered` directive | `unordered_fields.spec.yaml` |
| `$anywhere` directive | `anywhere_event.spec.yaml` |
| Multi-field capture | `multi_field_capture.spec.yaml` |
| Reordered field capture | `multi_field_capture_reordered.spec.yaml` |
| Inline checkpoint (`spec_trace!`) | `checkpoint_inline.spec.yaml` |

## Structured Values & Operators

| Feature | Fixture |
|---------|---------|
| List matching (exact, $contains, $size) | `structured_output.spec.yaml` |
| Map matching | `structured_map.spec.yaml` |
| Set matching | `structured_set.spec.yaml` |
| Nested list-of-maps | `nested_structured.spec.yaml` |
| Scalar operators ($gt, $lt, $matches, etc.) | `scalar_operators.spec.yaml` |
| Combined operators | `operators.spec.yaml` |

## Complex Inputs & Types

| Feature | Fixture |
|---------|---------|
| Struct deserialization (input) | `complex_inputs.spec.yaml` |
| Enum deserialization (input) | `complex_inputs.spec.yaml` |
| List/Map/Optional inputs | `complex_inputs.spec.yaml` |
| Nested struct round-trip | `complex_inputs.spec.yaml` |
| Enum event output | `enum_event.spec.yaml` |

## Property Tests

| Feature | Fixture |
|---------|---------|
| Basic property (commutativity) | `property_add.spec.yaml` |
| All generator types | `property_types.spec.yaml` |
| Counterexamples (failing props) | `property_counterexamples.spec.yaml` |
| Invalid: unknown generator type | `property_invalid.spec.yaml` |
| Invalid: inverted range | `property_invalid_range.spec.yaml` |
| Invalid: no generators | `property_no_generators.spec.yaml` |
| Invalid: no calls | `property_no_calls.spec.yaml` |
| Invalid: no $assert | `property_no_assert.spec.yaml` |
| Invalid: undefined ref | `property_bad_ref.spec.yaml` |

## Mocking

| Feature | Fixture |
|---------|---------|
| Mock field (dependency injection) | `mock_field.spec.yaml` |
| Mock with multiple responses | `mock_multi_response.spec.yaml` |
| Mock input not in table | `mock_not_found.spec.yaml` |

## Bindings & Targets

| Feature | Fixture |
|---------|---------|
| Target selection | `target_selection.spec.yaml` |
| Per-case target override | `per_case_target.spec.yaml` |
| Missing target (error) | `missing_target.spec.yaml` |
| Command target | `command_target.spec.yaml` |
| Cross-crate dependency | `cross_dep.spec.yaml` |

## Level & Provenance

| Feature | Fixture |
|---------|---------|
| `level: may` (skip if missing) | `level_may_missing.spec.yaml` |
| `level: should` (warn if missing) | `level_should_missing.spec.yaml` |
| Source provenance metadata | `provenance_example.spec.yaml` |

## Error Handling (harness errors)

| Feature | Fixture |
|---------|---------|
| Invalid YAML | `bad_yaml.spec.yaml` |
| Bad binding reference | `bad_binding.spec.yaml` |
| Missing operation | `missing_operation.spec.yaml` |
| Missing setup | `missing_setup.spec.yaml` |
| Compile error in source | `compile_error.spec.yaml` |
| No cases | `no_cases.spec.yaml` |
| Shape mismatch (undeclared output) | `shape_mismatch.spec.yaml` |

## Miscellaneous

| Feature | Fixture |
|---------|---------|
| Async operation | `async_fetch.spec.yaml` |
| Keyword collision (`run` as op name) | `keyword_collision.spec.yaml` |
| Vacuous match prevention | `vacuous_match.spec.yaml` |
| Expected mismatch — wrong value | `mismatch_wrong_field.spec.yaml` |
| Expected mismatch — missing event | `mismatch_missing_event.spec.yaml` |
| Expected mismatch — second step | `mismatch_second_step.spec.yaml` |
| Wrong result in stateful case | `statemachine_counter_wrong.spec.yaml` |
