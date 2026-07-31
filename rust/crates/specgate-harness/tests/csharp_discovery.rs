//! Focused C# discovery checks: call `discover` on a fixture spec and assert the
//! C# target self-describes to a schema identical to the Rust canonical.
//!
//! These build+run the C# fixture assembly via `dotnet`, so they are slow and
//! gated behind `--ignored`.

use specgate_harness::{DiscoverOutcome, TargetOutcome, discover};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/crates/specgate-harness
    p.pop(); // crates
    p.pop(); // rust
    p.pop(); // repo root
    p
}

fn discover_spec(rel: &str) -> DiscoverOutcome {
    let spec = repo_root().join(rel);
    discover(spec.to_str().expect("utf-8 path"))
}

fn assert_csharp_matches_canonical(rel: &str) {
    match discover_spec(rel) {
        DiscoverOutcome::Error { reason } => panic!("discover errored for {rel}: {reason}"),
        DiscoverOutcome::Complete { schema, targets } => {
            eprintln!("canonical schema for {rel}:\n{schema:#?}");
            let mut saw_csharp = false;
            for t in &targets {
                eprintln!("target {} => {:#?}", t.target, t.outcome);
                if t.target.starts_with("csharp") {
                    match &t.outcome {
                        TargetOutcome::SelfDescribed { schema: cs } => {
                            saw_csharp = true;
                            assert_eq!(cs, &schema, "C# target {} diverges from canonical", t.target);
                        }
                        TargetOutcome::NotSelfDescribing { reason } => {
                            panic!("C# target {} did not self-describe: {reason}", t.target)
                        }
                    }
                }
            }
            assert!(saw_csharp, "no csharp target found for {rel}");
        }
    }
}

#[test]
#[ignore = "builds C# via dotnet; slow"]
fn csharp_scalar_types_matches_canonical() {
    assert_csharp_matches_canonical("test/rust/crates/specgate-fixtures/specs/scalar_types.spec.yaml");
}

#[test]
#[ignore = "builds C# via dotnet; slow"]
fn csharp_all_discover_specs_match_canonical() {
    for rel in [
        "test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/async_fetch.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/void_operation.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/scalar_types.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/option_some.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/structured_map.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/structured_set.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/result_ok.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/structured_output.spec.yaml",
        "test/rust/crates/specgate-fixtures/specs/enum_event.spec.yaml",
    ] {
        assert_csharp_matches_canonical(rel);
    }
}
