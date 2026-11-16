# fixture.stateless_add

Rust adds opaque ignored contexts to the operation.
Replay must default them in native order without consuming recorded
`a` or `b`; the C# reference registry and traces remain unchanged.

Shared native scenarios (ordinary tests, not separately enumerated helpers):

- `StatelessAndMultiOperationFixturesPreserveArithmetic`: entry point owned by `fixture.stateless_add` in [BasicBehaviorTests.cs](csharp/tests/BasicBehaviorTests.cs); assertion steps execute in their original order.
