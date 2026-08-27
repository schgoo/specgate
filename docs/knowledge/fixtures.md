# Fixture Catalog

SpecGate's behavior is documented **by example**. Every supported feature has a
canonical, runnable fixture, and when this catalog (or any prose doc) disagrees
with a fixture, **the fixture is the source of truth**.

Most fixtures live under
[`test/rust/crates/specgate-fixtures/specs/`](../../test/rust/crates/specgate-fixtures/specs/).
Additional examples in dedicated crates are listed under
[Cross-crate examples](#cross-crate-examples).

## Cross-language conformance

Every fixture in this section is exercised by
[`specgate.conformance.spec.yaml`](../../specs/specgate.conformance.spec.yaml)
and is implemented for both Rust and C#/.NET. The harness requires both
bindings to self-describe the same normalized schema and emit the same canonical
trace.

C# 8 compatibility is sampled separately by `stateless_add`, `multi_setup`, and
`multi_mutation`; the complete matrix runs against the primary C# target.

### Basics

| Feature | Fixture |
|---------|---------|
| Simplest possible spec | [`stateless_add.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml) |
| Multiple cases in one spec | [`multi_case.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_case.spec.yaml) |
| Void operation (no return) | [`void_operation.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/void_operation.spec.yaml) |
| Read-only operation | [`readonly_operation.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/readonly_operation.spec.yaml) |
| Operations split across files | [`multi_file.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_file.spec.yaml) |
| Multiple top-level operations | [`multi_toplevel.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_toplevel.spec.yaml) |

### State and setup

| Feature | Fixture |
|---------|---------|
| Setup + stateful operation | [`statemachine_counter.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/statemachine_counter.spec.yaml) |
| Setup with input parameters | [`setup_with_params.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/setup_with_params.spec.yaml) |
| Multiple setups (multi-alias) | [`multi_setup.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_setup.spec.yaml) |
| Shared setup across operations | [`shared_setup.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/shared_setup.spec.yaml) |
| Multi-step case (sequential ops) | [`multi_step.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_step.spec.yaml) |
| Multiple field mutations | [`multi_mutation.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_mutation.spec.yaml) |
| Multiple field capture | [`multi_field_capture.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/multi_field_capture.spec.yaml) |
| Nested operations | [`nested_operations.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/nested_operations.spec.yaml) |

### Return and error types

| Feature | Fixture |
|---------|---------|
| Result — Ok path | [`result_ok.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/result_ok.spec.yaml) |
| Result — Error path | [`result_err.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/result_err.spec.yaml) |
| Option — Some path | [`option_some.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/option_some.spec.yaml) |
| Option — None path | [`option_none.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/option_none.spec.yaml) |
| Declared specific exception | [`checked_divide.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/checked_divide.spec.yaml) |
| Declared catch-all exception | [`catch_all.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/catch_all.spec.yaml) |
| Panic / unrecoverable | [`unrecoverable.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/unrecoverable.spec.yaml) |

### Traces and matching

| Feature | Fixture |
|---------|---------|
| Inline checkpoint | [`checkpoint_inline.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/checkpoint_inline.spec.yaml) |
| Scalar operators (`$gt`, `$lt`, `$matches`, etc.) | [`scalar_operators.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/scalar_operators.spec.yaml) |
| Combined assertion operators | [`operators.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/operators.spec.yaml) |

### Structured values and inputs

| Feature | Fixture |
|---------|---------|
| Primitive scalar types and universal `value` | [`scalar_types.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/scalar_types.spec.yaml) |
| List matching (exact, `$contains`, `$size`) | [`structured_output.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/structured_output.spec.yaml) |
| Map matching | [`structured_map.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/structured_map.spec.yaml) |
| Set matching | [`structured_set.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/structured_set.spec.yaml) |
| Nested list-of-maps | [`nested_structured.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/nested_structured.spec.yaml) |
| Struct, enum, list, map, and optional inputs | [`complex_inputs.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/complex_inputs.spec.yaml) |
| Renamed inputs | [`named_inputs.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/named_inputs.spec.yaml) |
| Optional inputs with `default:` | [`default_input.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/default_input.spec.yaml) |
| Enum event output | [`enum_event.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/enum_event.spec.yaml) |

### Mocking

| Feature | Fixture |
|---------|---------|
| Mock field (dependency injection) | [`mock_field.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/mock_field.spec.yaml) |
| Mock with multiple responses | [`mock_multi_response.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/mock_multi_response.spec.yaml) |
| Mock input not in table | [`mock_not_found.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/mock_not_found.spec.yaml) |

### Build, runtime, and translation

| Feature | Fixture |
|---------|---------|
| Async operation | [`async_fetch.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/async_fetch.spec.yaml) |
| Cross-crate / package dependency | [`cross_dep.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/cross_dep.spec.yaml) |
| Real-build execution (op exists only in compiled artifact) | [`realbuild_witness.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/realbuild_witness.spec.yaml) |
| Cross-target divergence detection | [`divergence_witness.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/divergence_witness.spec.yaml) |
| .NET component translated to Rust | [`order_pricing_translation.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/order_pricing_translation.spec.yaml) |

## Harness and toolchain behavior

Fixtures in this section test the harness, matcher, validation, target selection,
or Rust-specific tooling. They do **not** define the cross-language Rust/C# support
surface.

### Runner and matcher behavior

| Feature | Fixture |
|---------|---------|
| Setup with a side effect | [`side_effect_setup.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/side_effect_setup.spec.yaml) |
| Setup producing a simple output | [`simple_output_setup.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/simple_output_setup.spec.yaml) |
| `$anywhere` matcher behavior | [`anywhere_event.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/anywhere_event.spec.yaml) |
| Expected mismatch — wrong value | [`mismatch_wrong_field.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/mismatch_wrong_field.spec.yaml) |

### Property tests

| Feature | Fixture |
|---------|---------|
| Basic property (commutativity) | [`property_add.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_add.spec.yaml) |
| All generator types | [`property_types.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_types.spec.yaml) |
| Counterexamples (failing properties) | [`property_counterexamples.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_counterexamples.spec.yaml) |
| Invalid: unknown generator type | [`property_invalid.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_invalid.spec.yaml) |
| Invalid: inverted range | [`property_invalid_range.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_invalid_range.spec.yaml) |
| Invalid: no generators | [`property_no_generators.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_no_generators.spec.yaml) |
| Invalid: no calls | [`property_no_calls.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_no_calls.spec.yaml) |
| Invalid: no `$assert` | [`property_no_assert.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_no_assert.spec.yaml) |
| Invalid: undefined reference | [`property_bad_ref.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/property_bad_ref.spec.yaml) |

### Bindings and targets

| Feature | Fixture |
|---------|---------|
| Target selection | [`target_selection.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/target_selection.spec.yaml) |
| Per-case target override | [`per_case_target.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/per_case_target.spec.yaml) |
| Missing target (error) | [`missing_target.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/missing_target.spec.yaml) |
| Command target | [`command_target.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/command_target.spec.yaml) |

### Levels and provenance

| Feature | Fixture |
|---------|---------|
| `level: may` (skip if missing) | [`level_may_missing.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/level_may_missing.spec.yaml) |
| `level: should` (warn if missing) | [`level_should_missing.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/level_should_missing.spec.yaml) |
| Source provenance metadata | [`provenance_example.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/provenance_example.spec.yaml) |

### Validation and harness errors

| Feature | Fixture |
|---------|---------|
| Invalid YAML | [`bad_yaml.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/bad_yaml.spec.yaml) |
| Bad binding reference | [`bad_binding.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/bad_binding.spec.yaml) |
| Missing operation | [`missing_operation.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/missing_operation.spec.yaml) |
| Missing setup | [`missing_setup.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/missing_setup.spec.yaml) |
| Compile error in source | [`compile_error.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/compile_error.spec.yaml) |
| No cases | [`no_cases.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/no_cases.spec.yaml) |
| Shape mismatch (undeclared output) | [`shape_mismatch.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/shape_mismatch.spec.yaml) |

### Rust-specific runtime and tooling

| Feature | Fixture |
|---------|---------|
| Async — smol reactor timer | [`async_smol_timer.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/async_smol_timer.spec.yaml) |
| Async — tokio reactor timer (`runtime: tokio`) | [`async_tokio_timer.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/async_tokio_timer.spec.yaml) |
| Partial coverage measurement (`run --coverage`) | [`coverage_partial.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/coverage_partial.spec.yaml) |
| Keyword collision (`run` as operation name) | [`keyword_collision.spec.yaml`](../../test/rust/crates/specgate-fixtures/specs/keyword_collision.spec.yaml) |

## Cross-crate examples

These features are demonstrated by dedicated fixture crates rather than the main
corpus:

| Feature | Fixture crate (→ golden spec) |
|---------|-------------------------------|
| Built-in `value` type | [`specgate-value-fixture`](../../test/rust/crates/specgate-value-fixture/) → [`fixture.value.spec.yaml`](../../test/rust/crates/specgate-value-fixture/expected/fixture.value.spec.yaml) |
| Spec extraction — schema | [`specgate-extract-fixture`](../../test/rust/crates/specgate-extract-fixture/) → [`extracted.spec.yaml`](../../test/rust/crates/specgate-extract-fixture/expected/extracted.spec.yaml) |
| Spec extraction — `--cases` | [`specgate-cases-fixture`](../../test/rust/crates/specgate-cases-fixture/) → [`fixture.cases.spec.yaml`](../../test/rust/crates/specgate-cases-fixture/expected/fixture.cases.spec.yaml) |
| Components and `depends_on` | [`specgate-component-fixture`](../../test/rust/crates/specgate-component-fixture/) → [`comp.app`](../../test/rust/crates/specgate-component-fixture/expected/comp.app.spec.yaml), [`comp.core`](../../test/rust/crates/specgate-component-fixture/expected/comp.core.spec.yaml) |
| Public-API reachability (link-only) | [`nonpublic.spec.yaml`](../../test/fixtures/nonpublic/nonpublic.spec.yaml) — a private operation is rejected |
