// Error-case fixture: an annotated operation that is NOT publicly reachable.
// The module is `pub mod`-declared (so it resolves), but the operation function
// itself is private — so `use specgate_fixtures::engine::nonpublic_op::secret`
// cannot compile. Under the link-only harness this must surface as a CLEAN
// "operation is not publicly reachable" diagnostic (RunOutcome::Error), not a
// raw runner compile failure. Enforces the public-API contract: SpecGate only
// asserts on a component's public surface. Exercised by the harness spec case
// `nonpublic_operation_is_rejected` (input: test/fixtures/nonpublic/).
use specgate::*;

#[spec_operation("secret")]
fn secret() -> i32 {
    42
}
