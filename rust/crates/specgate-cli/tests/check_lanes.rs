//! Local `just check` lanes run in parallel and must not lease the same package.
//!
//! A `specgate` command leases each package it builds. A second command that
//! needs the same package fails at once. Each test binding therefore belongs
//! to exactly one lane, and no two lanes may resolve to the same `sources`
//! directory. A test may use only the bindings of its own lane.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Each lane and the bindings, relative to the repository root, that its tests use.
const LANES: &[(&str, &[&str])] = &[
    (
        "check-rust-replay",
        &[
            "rust/crates/specgate-cli/tests/fixtures/replay/specgate.yaml",
            "rust/crates/specgate-cli/tests/fixtures/arty-replay.yaml",
            "rust/crates/specgate-cli/tests/fixtures/private-replay.yaml",
            "rust/crates/specgate-cli/tests/fixtures/typed-async.yaml",
            "rust/crates/specgate-cli/tests/fixtures/typed-integration.yaml",
        ],
    ),
    (
        "check-rust-other",
        &[
            "rust/crates/specgate-cli/tests/fixtures/specgate.yaml",
            "rust/crates/specgate-cli/tests/fixtures/divergence.yaml",
            "rust/crates/specgate-cli/tests/fixtures/csharp-capture/specgate.yaml",
            "rust/crates/specgate-cli/tests/fixtures/csharp-capture/specgate-failing.yaml",
            "rust/crates/specgate-cli/tests/fixtures/csharp-capture/specgate-mstest.yaml",
            "rust/crates/specgate-cli/tests/fixtures/csharp-capture/specgate-nunit.yaml",
        ],
    ),
    ("check-ctsc", &["test/behavior/specgate.yaml"]),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_path_buf()
}

fn yaml_files(directory: &Path, found: &mut BTreeSet<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            yaml_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "yaml") {
            found.insert(path);
        }
    }
}

#[test]
fn every_cli_fixture_binding_belongs_to_one_lane() {
    let root = repo_root();
    let assigned: Vec<PathBuf> = LANES
        .iter()
        .flat_map(|(_, bindings)| bindings.iter().map(|binding| root.join(binding)))
        .collect();
    let unique: BTreeSet<_> = assigned.iter().cloned().collect();
    assert_eq!(unique.len(), assigned.len(), "a binding is listed in more than one lane");
    let mut found = BTreeSet::new();
    yaml_files(&root.join("rust/crates/specgate-cli/tests/fixtures"), &mut found);
    let unassigned: Vec<_> = found.difference(&unique).collect();
    assert!(unassigned.is_empty(), "assign these bindings to a lane in LANES: {unassigned:#?}");
}

#[test]
fn lanes_never_lease_the_same_package() {
    let root = repo_root();
    let mut owners: BTreeMap<PathBuf, &str> = BTreeMap::new();
    for (lane, bindings) in LANES {
        for binding in *bindings {
            let binding =
                specgate_discovery::binding::load_binding(root.join(binding)).unwrap_or_else(|error| panic!("load {binding}: {error}"));
            for implementation in binding.components.values().flat_map(BTreeMap::values) {
                let sources = std::fs::canonicalize(&implementation.sources)
                    .unwrap_or_else(|error| panic!("resolve {}: {error}", implementation.sources.display()));
                let owner = *owners.entry(sources.clone()).or_insert(lane);
                assert_eq!(owner, *lane, "{} is leased by both {owner} and {lane}", sources.display());
            }
        }
    }
}
