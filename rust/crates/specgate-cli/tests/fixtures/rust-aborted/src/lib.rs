//! Capture corpus for a target that ends the process inside an open operation.
//!
//! Capture runs one execution pass per binding over every test in the target
//! package. This shape produces no capture by design, so it needs a package of
//! its own: the ordinary `fixture.cli` corpus is a golden-matrix row, and
//! golden generation captures strictly, where any enumerated test that yields
//! no scenario fails the batch.
//!
//! Discovery is source-based per package, so `fixture.cli.aborted` is declared
//! here rather than reused from `specgate-cli-fixtures`. The registry identity
//! is deliberately the same as the sibling declaration in that crate; the two
//! bindings never resolve into one registry.
//!
//! `fixture.cli.survivor` exists only to prove isolation: an unfinalizable
//! recording must cost its own scenario and nothing else, so a sibling
//! component enumerated in the same pass must still bundle normally.

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

/// Completes ordinarily, in the same execution pass as [`abort_inside_operation`].
#[spec_operation("survive", spec = "fixture.cli.survivor")]
pub fn survive(value: i32) -> i32 {
    value * 2
}

#[cfg(test)]
mod tests {
    use super::{abort_inside_operation, survive};

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
    /// finalized, so the CLI drops this scenario rather than emitting a bundle
    /// that reports `error` for a test that passed — see
    /// `docs/decisions/top-level-abandonment-unsupported.md`.
    #[test]
    fn deliberately_exits_inside_an_open_operation() {
        assert_eq!(abort_inside_operation(1), -1, "unreachable: the operation exits the process");
    }

    /// Records a finalized scenario for `fixture.cli.survivor`.
    ///
    /// Unaffected by the aborting test above: capture runs each enumerated test
    /// in its own process with `--exact`, so the exit cannot reach this one.
    #[test]
    fn survives_alongside_the_aborting_test() {
        assert_eq!(survive(21), 42);
    }
}
