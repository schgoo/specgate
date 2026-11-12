# fixture.structured_output

Shared native scenarios (ordinary tests, not separately enumerated helpers):

- `ScalarAndStructuredRecordsRemainDistinct`: entry point owned by `fixture.scalar_types` in [StructuredBehaviorTests.cs](../fixture.scalar_types/csharp/tests/StructuredBehaviorTests.cs); assertion steps execute in their original order.

The C# `EntityType` inherits its first annotated property (`entity_name`) from
an unannotated base class. Its remaining member order, registry, scenario
identity, and replay surface against the unchanged Rust candidate stay the same.
