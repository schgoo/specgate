//! Capture corpus for a target that ends the process inside an open operation.
//!
//! Capture runs one execution pass per binding over every test in the target
//! package, and this shape makes that pass fail by design, so it needs a
//! package of its own: enumerating it beside the ordinary `fixture.cli` corpus
//! would fail every capture of that corpus too.
//!
//! Discovery is source-based per package, so `fixture.cli.aborted` is declared
//! here rather than reused from `specgate-cli-fixtures`. The registry identity
//! is deliberately the same as the sibling declaration in that crate; the two
//! bindings never resolve into one registry.

use specgate::spec_operation;

specgate::spec_component!("fixture.cli");

/// Ends the process from inside its own open operation scope.
///
/// Mirrors `specgate_cli_fixtures::abort_inside_operation`.
#[spec_operation("abort", spec = "fixture.cli.aborted")]
pub fn abort_inside_operation(value: i32) -> i32 {
    if value >= 0 {
        std::process::exit(0);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::abort_inside_operation;

    /// Leaves `abort`'s operation scope open while the process reports success.
    ///
    /// Exits before the assertion. Safe because this crate is its own workspace
    /// and is never part of the ordinary `cargo test` run; capture runs each
    /// test in its own process with `--exact`.
    ///
    /// This is the `specgate capture` shape of issue #55 and needs no test-only
    /// bypass: `std::process::exit` runs no destructor, so the macro-owned
    /// `OperationScope` never closes, yet the process exits 0 and capture sees
    /// an ordinary passing test. The resulting recording can never be
    /// finalized, so the CLI rejects it rather than emitting a bundle that
    /// reports `error` for a test that passed — see
    /// `docs/decisions/top-level-abandonment-unsupported.md`.
    #[test]
    fn deliberately_exits_inside_an_open_operation() {
        assert_eq!(abort_inside_operation(1), -1, "unreachable: the operation exits the process");
    }
}
