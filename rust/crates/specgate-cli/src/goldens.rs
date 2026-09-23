//! CTSC golden matrix harness.
//!
//! The checked-in matrix at `test/goldens/ctsc/matrix.json` is the reviewable
//! configuration; every artifact beside it is regenerated product output. The
//! harness discovers, captures, and validates the whole fixture corpus in one
//! pass, then either rewrites the goldens (`update`) or byte-compares fresh
//! output against them (`check`).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use specgate_ctsc::comparison::compare;
use specgate_ctsc::encode_schema_registry_result;
use specgate_ctsc::validation::{validate_bundle, validate_linked, validate_registry, validate_trace};
use specgate_discovery::binding::resolve_binding_target;
use specgate_discovery::discovery::{Registry, cargo_bin, discover_many_target, normalize_registry};

use crate::capture::{CaptureRequest, capture_discovered_strict, discover_capture_target};
use crate::replay::{ReplayCandidates, replay_failure_category};

const MATRIX_FILE: &str = "matrix.json";
const REGISTRY_FILE: &str = "registry.ctsc.json";
const TRACE_FILE: &str = "reference.otlp.json";
const MANIFEST_FILE: &str = "manifest.json";
const ERROR_FILE: &str = "error.json";
const REGISTRY_VERSION: &str = "0.1.0";

const CLASS_COMPONENT: &str = "implementation-component";
const CLASS_NEGATIVE: &str = "negative-fixture";
const CLASS_REPLACEMENT: &str = "replacement";

/// The limitation code a row declares when its component is async and can
/// therefore only be discovered, never captured.
const ASYNC_LIMITATION: &str = "async-capture-unsupported";
const REPLAY_FAILURE_CATEGORIES: &[&str] = &[
    "structured-value",
    "setup-backed-operation",
    "method-operation",
    "async-operation",
    "unsupported-language",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Update,
    Check,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Matrix {
    format: String,
    format_version: String,
    artifact_root: String,
    bindings: BTreeMap<String, String>,
    sources: SourceCoverage,
    rows: Vec<Row>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SourceCoverage {
    roots: Vec<String>,
    exclusions: Vec<Exclusion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Exclusion {
    rule: String,
    description: String,
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Row {
    id: String,
    classification: String,
    source_group: String,
    #[serde(default)]
    component: Option<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    rust: Option<LanguageRow>,
    #[serde(default)]
    csharp: Option<LanguageRow>,
    parity: Parity,
    replay: Replay,
    #[serde(default)]
    linkage: Option<Linkage>,
    #[serde(default)]
    limitations: Vec<Limitation>,
    #[serde(default)]
    capture_exclusions: Vec<CaptureExclusion>,
    #[serde(default)]
    replacement: Option<Replacement>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LanguageRow {
    binding: String,
    phases: Vec<String>,
    expect: String,
    artifacts: Vec<String>,
    #[serde(default)]
    expect_category: Option<String>,
    #[serde(default)]
    feature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Parity {
    mode: String,
    #[serde(default)]
    reason: Option<String>,
    /// Every semantic difference a `exception` row is allowed to have, as an
    /// exact machine-readable set. The observed Rust-vs-C# diff must equal this
    /// set, so a new, missing, or changed difference fails the gate.
    #[serde(default)]
    allowed_differences: Vec<ParityDifference>,
}

/// One declared or observed semantic difference between the Rust and C#
/// registry documents of a component.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ParityDifference {
    /// JSON Pointer into the registry document.
    path: String,
    rust: ParitySide,
    csharp: ParitySide,
}

/// One side of a semantic difference.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "camelCase")]
enum ParitySide {
    /// The language does not emit this member at all.
    Missing,
    /// Both languages emit an array here, with different lengths.
    ArrayLength { length: usize },
    /// The language emits this exact value.
    Value { value: serde_json::Value },
}

impl ParityDifference {
    fn render(&self) -> String {
        serde_json::to_string(self).expect("parity difference serializes")
    }
}

/// Every semantic difference between two registry documents, in a stable order.
///
/// Walks both documents together: object members are compared by name (a member
/// only one side emits is a `missing` difference), arrays report a length
/// difference and then compare their common prefix element-wise, and any other
/// inequality is reported as the two concrete values. Paths are JSON Pointers,
/// so a declared exception pins the exact operation, type, and variant it
/// covers; adding or removing an operation or type shifts or adds paths and no
/// longer matches the declaration.
fn semantic_differences(rust: &serde_json::Value, csharp: &serde_json::Value) -> Vec<ParityDifference> {
    let mut differences = Vec::new();
    collect_differences(rust, csharp, "", &mut differences);
    differences
}

fn collect_differences(rust: &serde_json::Value, csharp: &serde_json::Value, path: &str, out: &mut Vec<ParityDifference>) {
    if rust == csharp {
        return;
    }
    match (rust, csharp) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            let names = left.keys().chain(right.keys()).cloned().collect::<BTreeSet<_>>();
            for name in names {
                let child = format!("{path}/{}", escape_pointer_token(&name));
                match (left.get(&name), right.get(&name)) {
                    (Some(left_value), Some(right_value)) => collect_differences(left_value, right_value, &child, out),
                    (Some(left_value), None) => out.push(ParityDifference {
                        path: child,
                        rust: ParitySide::Value { value: left_value.clone() },
                        csharp: ParitySide::Missing,
                    }),
                    (None, Some(right_value)) => out.push(ParityDifference {
                        path: child,
                        rust: ParitySide::Missing,
                        csharp: ParitySide::Value {
                            value: right_value.clone(),
                        },
                    }),
                    (None, None) => unreachable!("name came from one of the two objects"),
                }
            }
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            if left.len() != right.len() {
                out.push(ParityDifference {
                    path: path.to_string(),
                    rust: ParitySide::ArrayLength { length: left.len() },
                    csharp: ParitySide::ArrayLength { length: right.len() },
                });
            }
            for (index, (left_value, right_value)) in left.iter().zip(right.iter()).enumerate() {
                collect_differences(left_value, right_value, &format!("{path}/{index}"), out);
            }
        }
        _ => out.push(ParityDifference {
            path: path.to_string(),
            rust: ParitySide::Value { value: rust.clone() },
            csharp: ParitySide::Value { value: csharp.clone() },
        }),
    }
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Replay {
    mode: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    expect_category: Option<String>,
}

/// How a captured bundle relates to its own registry.
///
/// Every captured Rust bundle is `linked`: it passes native trace, Linked, and
/// bundle validation against the registry discovered from the same sources.
/// `not-applicable` belongs to discovery-only rows, which own no bundle. There
/// is no exception mode — a bundle that cannot link is a product defect.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Linkage {
    mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Limitation {
    code: String,
    detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CaptureExclusion {
    operation: String,
    code: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Replacement {
    feature: String,
    rationale: String,
    references: Vec<Reference>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Reference {
    path: String,
    test: String,
}

impl Row {
    fn rust_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.id).join("rust")
    }

    fn csharp_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.id).join("csharp")
    }

    fn linkage_mode(&self) -> &str {
        self.linkage.as_ref().map_or("not-applicable", |linkage| linkage.mode.as_str())
    }

    fn captures_rust(&self) -> bool {
        self.rust
            .as_ref()
            .is_some_and(|language| language.phases.iter().any(|phase| phase == "capture"))
    }
}

fn repo_root() -> PathBuf {
    std::env::current_dir()
        .expect("current directory")
        .ancestors()
        .find(|path| path.join("rust").join("Cargo.toml").is_file() && path.join("justfile").is_file())
        .expect("repository root")
        .to_path_buf()
}

fn repo_path(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    path
}

fn read_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    let parent = path.parent().expect("artifact parent");
    std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}

fn load_matrix(root: &Path) -> Matrix {
    let path = repo_path(root, "test/goldens/ctsc").join(MATRIX_FILE);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let matrix: Matrix =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{} is not a valid golden matrix: {error}", path.display()));
    assert_eq!(matrix.format, "specgate.ctsc-golden-matrix", "unexpected matrix format");
    assert_eq!(matrix.format_version, "0.1.0", "unexpected matrix format version");
    assert_eq!(matrix.artifact_root, "test/goldens/ctsc", "unexpected matrix artifact root");
    matrix
}
// ---------------------------------------------------------------------------
// Matrix shape, source coverage, and replacement references
// ---------------------------------------------------------------------------

fn check_matrix_shape(matrix: &Matrix) {
    let mut ids = BTreeSet::new();
    let mut components = BTreeSet::new();
    for row in &matrix.rows {
        assert!(ids.insert(row.id.clone()), "duplicate matrix row id '{}'", row.id);
        assert!(
            !row.id.starts_with('/') && !row.id.ends_with('/') && !row.id.contains("..") && !row.id.contains('\\'),
            "matrix row id '{}' must be a relative forward-slash artifact path",
            row.id
        );
        assert!(
            matches!(row.classification.as_str(), CLASS_COMPONENT | CLASS_NEGATIVE | CLASS_REPLACEMENT),
            "row '{}' has unknown classification '{}'",
            row.id,
            row.classification
        );
        assert!(!row.source_group.is_empty(), "row '{}' must declare a source group", row.id);
        assert!(
            matches!(
                row.parity.mode.as_str(),
                "byte-identical" | "exception" | "rust-only" | "not-applicable"
            ),
            "row '{}' has unknown parity mode '{}'",
            row.id,
            row.parity.mode
        );
        assert!(
            matches!(
                row.replay.mode.as_str(),
                "verified" | "link-only" | "unsupported" | "not-applicable"
            ),
            "row '{}' has unknown replay mode '{}'",
            row.id,
            row.replay.mode
        );
        assert_eq!(
            row.captures_rust(),
            row.replay.mode != "not-applicable",
            "row '{}' must declare a replay mode exactly when it captures a Rust bundle",
            row.id
        );
        if row.parity.mode != "byte-identical" {
            assert!(
                row.parity.reason.as_ref().is_some_and(|reason| !reason.is_empty()),
                "row '{}' parity mode '{}' requires an explicit reason",
                row.id,
                row.parity.mode
            );
        }
        if row.parity.mode == "exception" {
            assert!(
                !row.parity.allowed_differences.is_empty(),
                "row '{}' is a parity exception and must declare the exact differences it allows",
                row.id
            );
            let mut declared = BTreeSet::new();
            for difference in &row.parity.allowed_differences {
                assert!(
                    difference.path.starts_with('/'),
                    "row '{}' declares parity difference path '{}', which is not a JSON Pointer",
                    row.id,
                    difference.path
                );
                assert!(
                    difference.rust != difference.csharp,
                    "row '{}' declares an identical parity difference at '{}'",
                    row.id,
                    difference.path
                );
                assert!(
                    declared.insert(difference.render()),
                    "row '{}' declares the same parity difference twice at '{}'",
                    row.id,
                    difference.path
                );
            }
        } else {
            assert!(
                row.parity.allowed_differences.is_empty(),
                "row '{}' declares parity differences but its mode is '{}'",
                row.id,
                row.parity.mode
            );
        }
        if row.replay.mode != "verified" {
            assert!(
                row.replay.reason.as_ref().is_some_and(|reason| !reason.is_empty()),
                "row '{}' replay mode '{}' requires an explicit reason",
                row.id,
                row.replay.mode
            );
        }
        if row.replay.mode == "unsupported" {
            let category = row
                .replay
                .expect_category
                .as_deref()
                .unwrap_or_else(|| panic!("row '{}' marks replay unsupported and must declare expectCategory", row.id));
            assert!(
                REPLAY_FAILURE_CATEGORIES.contains(&category),
                "row '{}' declares unknown replay failure category '{category}'",
                row.id
            );
        } else {
            assert!(
                row.replay.expect_category.is_none(),
                "row '{}' declares replay expectCategory but its mode is '{}'",
                row.id,
                row.replay.mode
            );
        }
        assert!(
            matches!(row.linkage_mode(), "linked" | "not-applicable"),
            "row '{}' has unknown linkage mode '{}'",
            row.id,
            row.linkage_mode()
        );
        assert_eq!(
            row.captures_rust(),
            row.linkage_mode() == "linked",
            "row '{}' must declare linkage 'linked' exactly when it captures a Rust bundle",
            row.id
        );
        for language in [row.rust.as_ref(), row.csharp.as_ref()].into_iter().flatten() {
            assert!(
                matrix.bindings.contains_key(&language.binding),
                "row '{}' references unknown binding '{}'",
                row.id,
                language.binding
            );
            assert!(
                matches!(language.expect.as_str(), "success" | "failure"),
                "row '{}' has unknown expectation '{}'",
                row.id,
                language.expect
            );
            assert!(!language.phases.is_empty(), "row '{}' must declare at least one phase", row.id);
            for phase in &language.phases {
                assert!(
                    matches!(phase.as_str(), "discover" | "capture" | "build"),
                    "row '{}' has unknown phase '{}'",
                    row.id,
                    phase
                );
            }
            if language.expect == "failure" {
                assert!(
                    language.expect_category.is_some(),
                    "row '{}' expects failure and must declare expectCategory",
                    row.id
                );
            }
        }
        match row.classification.as_str() {
            CLASS_COMPONENT => {
                let component = row
                    .component
                    .as_ref()
                    .unwrap_or_else(|| panic!("row '{}' must name a component", row.id));
                assert!(
                    components.insert(component.clone()),
                    "component '{component}' is covered by more than one matrix row"
                );
                assert_eq!(
                    row.id,
                    format!("component/{component}"),
                    "component row id must be 'component/<component>'"
                );
                let rust = row
                    .rust
                    .as_ref()
                    .unwrap_or_else(|| panic!("component row '{}' must declare Rust coverage", row.id));
                assert!(!row.sources.is_empty(), "component row '{}' must list its sources", row.id);
                let expected_rust_artifacts = if row.captures_rust() {
                    vec![MANIFEST_FILE.to_string(), TRACE_FILE.to_string(), REGISTRY_FILE.to_string()]
                } else {
                    vec![REGISTRY_FILE.to_string()]
                };
                assert_eq!(
                    rust.artifacts, expected_rust_artifacts,
                    "component row '{}' declares the wrong Rust artifact set",
                    row.id
                );
                if let Some(csharp) = row.csharp.as_ref() {
                    assert_eq!(
                        csharp.artifacts,
                        vec![REGISTRY_FILE.to_string()],
                        "C# coverage is registry-only; row '{}' declares more",
                        row.id
                    );
                    assert_eq!(
                        csharp.phases,
                        vec!["discover".to_string()],
                        "C# coverage is discovery-only; row '{}' declares more",
                        row.id
                    );
                }
            }
            CLASS_NEGATIVE => {
                assert!(row.id.starts_with("negative/"), "negative row id must start with 'negative/'");
                assert!(!row.sources.is_empty(), "negative row '{}' must list its sources", row.id);
                let language = row
                    .rust
                    .as_ref()
                    .unwrap_or_else(|| panic!("negative row '{}' needs Rust coverage", row.id));
                assert_eq!(language.expect, "failure", "negative row '{}' must expect failure", row.id);
                assert_eq!(
                    language.artifacts,
                    vec![ERROR_FILE.to_string()],
                    "negative row '{}' must produce exactly {ERROR_FILE}",
                    row.id
                );
            }
            _ => {
                assert!(
                    row.id.starts_with("replacement/"),
                    "replacement row id must start with 'replacement/'"
                );
                assert!(
                    row.rust.is_none() && row.csharp.is_none(),
                    "replacement row '{}' owns no artifacts",
                    row.id
                );
                let replacement = row
                    .replacement
                    .as_ref()
                    .unwrap_or_else(|| panic!("replacement row '{}' must describe what it replaces", row.id));
                assert!(!replacement.feature.is_empty(), "replacement row '{}' needs a feature", row.id);
                assert!(!replacement.rationale.is_empty(), "replacement row '{}' needs a rationale", row.id);
                assert!(
                    !replacement.references.is_empty(),
                    "replacement row '{}' must point at CTSC-native tests",
                    row.id
                );
            }
        }
        for limitation in &row.limitations {
            assert!(
                !limitation.code.is_empty() && !limitation.detail.is_empty(),
                "row '{}' declares an empty limitation",
                row.id
            );
            assert!(
                limitation.code != ASYNC_LIMITATION || !row.captures_rust(),
                "row '{}' declares '{ASYNC_LIMITATION}' and must stay discovery-only",
                row.id
            );
        }
        assert!(
            row.capture_exclusions.is_empty() || row.classification == CLASS_COMPONENT && row.captures_rust(),
            "row '{}' may declare captureExclusions only for a captured component",
            row.id
        );
        let mut excluded_operations = BTreeSet::new();
        for exclusion in &row.capture_exclusions {
            assert!(
                !exclusion.operation.is_empty() && is_stable_code(&exclusion.code) && !exclusion.reason.is_empty(),
                "row '{}' declares an invalid capture exclusion; operation, stable code, and reason are required",
                row.id
            );
            assert!(
                excluded_operations.insert(exclusion.operation.as_str()),
                "row '{}' excludes capture of operation '{}' more than once",
                row.id,
                exclusion.operation
            );
        }
    }
}

fn is_stable_code(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_source_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let stem = name.strip_suffix(".in").unwrap_or(name);
    Path::new(stem)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "rs" | "cs"))
}

fn collect_sources(root: &Path, relative_root: &str) -> Vec<String> {
    let mut found = Vec::new();
    let start = repo_path(root, relative_root);
    let mut stack = vec![start];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory).unwrap_or_else(|error| panic!("failed to list {}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if matches!(name.as_str(), "bin" | "obj" | "target") {
                    continue;
                }
                stack.push(path);
            } else if is_source_file(&path) {
                found.push(relative_display(root, &path));
            }
        }
    }
    found.sort();
    found
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|_error| panic!("{} is outside the repository", path.display()))
        .to_string_lossy()
        .replace('\\', "/")
}

fn declares_operation(text: &str) -> bool {
    text.contains("spec_operation(") || text.contains("[SpecOperation(")
}

fn declares_any_annotation(text: &str) -> bool {
    declares_operation(text)
        || [
            "spec_setup(",
            "spec_component!(",
            "SpecEvent",
            "[SpecSetup(",
            "[SpecEvent(",
            "[SpecException(",
            "[SpecInput(",
        ]
        .iter()
        .any(|token| text.contains(token))
}

fn check_source_coverage(root: &Path, matrix: &Matrix) {
    let covered = matrix
        .rows
        .iter()
        .flat_map(|row| row.sources.iter().map(|source| (source.clone(), row.id.clone())))
        .collect::<Vec<_>>();
    let mut covered_paths = BTreeMap::new();
    for (source, row_id) in covered {
        covered_paths.entry(source).or_insert_with(Vec::new).push(row_id);
    }
    let excluded = matrix
        .sources
        .exclusions
        .iter()
        .flat_map(|exclusion| exclusion.paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    for exclusion in &matrix.sources.exclusions {
        assert!(
            !exclusion.rule.is_empty() && !exclusion.description.is_empty(),
            "every source exclusion must state its rule and why it applies"
        );
    }
    assert!(
        matrix
            .sources
            .exclusions
            .iter()
            .any(|exclusion| exclusion.rule == "no-spec-annotation"),
        "the matrix must encode the rule that unannotated sources are out of scope"
    );

    let mut universe = BTreeSet::new();
    for source_root in &matrix.sources.roots {
        universe.extend(collect_sources(root, source_root));
    }

    let mut problems = Vec::new();
    for source in &universe {
        let text = std::fs::read_to_string(repo_path(root, source)).unwrap_or_default();
        let annotated = declares_operation(&text);
        let covered = covered_paths.contains_key(source);
        if annotated && !covered {
            problems.push(format!("{source}: declares operations but no matrix row lists it as a source"));
        }
        if !annotated && covered {
            problems.push(format!("{source}: is listed as a matrix source but declares no operation"));
        }
        if !annotated && !covered && !excluded.contains(source) && declares_any_annotation(&text) {
            problems.push(format!(
                "{source}: carries spec annotations but is neither covered by a row nor an explicit matrix exclusion"
            ));
        }
    }
    for source in covered_paths.keys() {
        if !universe.contains(source) {
            problems.push(format!("{source}: matrix source does not exist under any declared source root"));
        }
    }
    for source in &excluded {
        if !universe.contains(source) {
            problems.push(format!("{source}: matrix exclusion does not exist under any declared source root"));
        }
    }
    assert!(
        problems.is_empty(),
        "matrix source coverage is incomplete:\n  {}",
        problems.join("\n  ")
    );
}

fn check_replacement_references(root: &Path, matrix: &Matrix) {
    let mut problems = Vec::new();
    for row in matrix.rows.iter().filter(|row| row.classification == CLASS_REPLACEMENT) {
        let replacement = row.replacement.as_ref().expect("validated replacement");
        for reference in &replacement.references {
            let path = repo_path(root, &reference.path);
            if !path.is_file() {
                problems.push(format!("{}: {} does not exist", row.id, reference.path));
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let declared = text.contains(&format!("fn {}(", reference.test)) || text.contains(&format!("void {}(", reference.test));
            if !declared {
                problems.push(format!("{}: {} does not declare test '{}'", row.id, reference.path, reference.test));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "replacement rows must point at existing CTSC-native tests:\n  {}",
        problems.join("\n  ")
    );
}
// ---------------------------------------------------------------------------
// Artifact generation
// ---------------------------------------------------------------------------

fn registry_id(component: &str) -> String {
    format!("urn:ctsc:registry:{component}")
}

fn encode_registry(schema: &specgate_discovery::discovery::DiscoveredSchema, component: &str) -> Result<Vec<u8>, String> {
    let schema_json = serde_json::to_string(schema).map_err(|error| format!("failed to serialize discovery schema: {error}"))?;
    let encoded = encode_schema_registry_result(registry_id(component), REGISTRY_VERSION.to_string(), &schema_json)?;
    Ok(encoded.registry_json.into_bytes())
}

fn generate_rust_binding(root: &Path, matrix: &Matrix, out_root: &Path, binding_key: &str) {
    let rows = matrix
        .rows
        .iter()
        .filter(|row| row.classification == CLASS_COMPONENT)
        .filter(|row| row.rust.as_ref().is_some_and(|language| language.binding == binding_key))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    let binding = repo_path(root, &matrix.bindings[binding_key]);
    let binding = binding.to_str().expect("utf-8 binding path");

    let discovered = discover_capture_target(binding, "").unwrap_or_else(|error| panic!("{binding_key}: discovery failed: {error}"));
    let present = discovered.registry.present_components();
    for row in &rows {
        let component = row.component.as_ref().expect("component row");
        assert!(
            present.iter().any(|candidate| candidate == component),
            "{binding_key}: matrix component '{component}' is absent from discovery; available: {}",
            present.join(", ")
        );
    }
    let mut unmatched = present
        .iter()
        .filter(|component| !rows.iter().any(|row| row.component.as_deref() == Some(component.as_str())))
        .cloned()
        .collect::<Vec<_>>();
    unmatched.sort();
    assert!(
        unmatched.is_empty(),
        "{binding_key}: discovered components missing from the matrix: {}",
        unmatched.join(", ")
    );
    assert_capture_rows_are_synchronous(binding_key, &discovered.registry, &rows);

    for row in rows.iter().filter(|row| !row.captures_rust()) {
        let component = row.component.as_ref().expect("component row");
        let schema = normalize_registry(&discovered.registry, &discovered.target.language, component)
            .unwrap_or_else(|error| panic!("{component}: discovery-only normalization failed: {error}"));
        let bytes = encode_registry(&schema, component).unwrap_or_else(|error| panic!("{component}: registry encoding failed: {error}"));
        write_bytes(&row.rust_dir(out_root).join(REGISTRY_FILE), &bytes);
    }

    let requests = rows
        .iter()
        .filter(|row| row.captures_rust())
        .map(|row| CaptureRequest {
            component: row.component.clone().expect("component row"),
            out: row.rust_dir(out_root),
            excluded_operations: row.capture_exclusions.iter().map(|exclusion| exclusion.operation.clone()).collect(),
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return;
    }
    let reports =
        capture_discovered_strict(&discovered, &requests).unwrap_or_else(|error| panic!("{binding_key}: batched capture failed: {error}"));
    assert_eq!(
        reports.len(),
        requests.len(),
        "{binding_key}: capture returned the wrong bundle count"
    );
    assert_capture_operation_coverage(binding_key, &discovered.registry, &rows, out_root);
}

type OperationIdentity = (String, String);

/// Fail unless every discovered operation is represented by at least one
/// captured span, including operations reached only through nested calls.
///
/// Exclusions are exact semantic operation identities. They must name a real,
/// currently unobserved operation; an unknown or newly covered exclusion is
/// stale and fails rather than weakening the gate indefinitely.
fn assert_capture_operation_coverage(binding_key: &str, registry: &Registry, rows: &[&Row], out_root: &Path) {
    let mut problems = Vec::new();
    for row in rows.iter().filter(|row| row.captures_rust()) {
        let component = row.component.as_deref().expect("component row");
        let discovered = registry
            .ops
            .iter()
            .filter(|operation| !operation.is_setup && operation.component == component)
            .map(|operation| (operation.component.clone(), operation.name.clone()))
            .collect::<BTreeSet<_>>();
        let captured = captured_operation_identities(&row.rust_dir(out_root))
            .unwrap_or_else(|error| panic!("{}: failed to inspect captured operation coverage: {error}", row.id));
        let excluded = row
            .capture_exclusions
            .iter()
            .map(|exclusion| (component.to_string(), exclusion.operation.clone()))
            .collect::<BTreeSet<_>>();
        problems.extend(capture_coverage_problems(&row.id, &discovered, &captured, &excluded));
    }
    assert!(
        problems.is_empty(),
        "{binding_key}: golden capture operation coverage failed:\n  {}",
        problems.join("\n  ")
    );
}

fn captured_operation_identities(bundle_dir: &Path) -> Result<BTreeSet<OperationIdentity>, String> {
    let trace = std::fs::read(bundle_dir.join(TRACE_FILE)).map_err(|error| format!("failed to read {TRACE_FILE}: {error}"))?;
    let document: serde_json::Value = serde_json::from_slice(&trace).map_err(|error| format!("{TRACE_FILE} is not valid JSON: {error}"))?;
    let resources = document["resourceSpans"]
        .as_array()
        .ok_or_else(|| format!("{TRACE_FILE} has no resourceSpans array"))?;
    let mut operations = BTreeSet::new();
    for resource in resources {
        let scopes = resource["scopeSpans"]
            .as_array()
            .ok_or_else(|| format!("{TRACE_FILE} resource has no scopeSpans array"))?;
        for scope in scopes {
            let spans = scope["spans"]
                .as_array()
                .ok_or_else(|| format!("{TRACE_FILE} scope has no spans array"))?;
            for span in spans.iter().filter(|span| span["name"].as_str() == Some("conformance.operation")) {
                let attributes = span["attributes"]
                    .as_array()
                    .ok_or_else(|| "conformance.operation span has no attributes array".to_string())?;
                let attribute = |key: &str| {
                    attributes
                        .iter()
                        .find(|attribute| attribute["key"].as_str() == Some(key))
                        .and_then(|attribute| attribute["value"]["stringValue"].as_str())
                        .map(str::to_string)
                        .ok_or_else(|| format!("conformance.operation span has no string attribute '{key}'"))
                };
                operations.insert((attribute("conformance.component.id")?, attribute("conformance.operation.name")?));
            }
        }
    }
    Ok(operations)
}

fn capture_coverage_problems(
    row_id: &str,
    discovered: &BTreeSet<OperationIdentity>,
    captured: &BTreeSet<OperationIdentity>,
    excluded: &BTreeSet<OperationIdentity>,
) -> Vec<String> {
    let missing = discovered
        .difference(captured)
        .filter(|operation| !excluded.contains(*operation))
        .map(render_operation_identity)
        .collect::<Vec<_>>();
    let unexpected = captured.difference(discovered).map(render_operation_identity).collect::<Vec<_>>();
    let unknown_exclusions = excluded.difference(discovered).map(render_operation_identity).collect::<Vec<_>>();
    let stale_exclusions = excluded.intersection(captured).map(render_operation_identity).collect::<Vec<_>>();

    let mut problems = Vec::new();
    if !missing.is_empty() {
        problems.push(format!(
            "{row_id}: discovered operations have no captured trace: [{}]",
            missing.join(", ")
        ));
    }
    if !unexpected.is_empty() {
        problems.push(format!(
            "{row_id}: captured traces contain undiscovered operations: [{}]",
            unexpected.join(", ")
        ));
    }
    if !unknown_exclusions.is_empty() {
        problems.push(format!(
            "{row_id}: capture exclusions name undiscovered operations: [{}]",
            unknown_exclusions.join(", ")
        ));
    }
    if !stale_exclusions.is_empty() {
        problems.push(format!(
            "{row_id}: capture exclusions are stale because the operations were captured: [{}]",
            stale_exclusions.join(", ")
        ));
    }
    problems
}

fn render_operation_identity((component, operation): &OperationIdentity) -> String {
    format!("{component}::{operation}")
}

/// Fail unless every row that captures a Rust bundle is synchronous or
/// explicitly excludes each asynchronous operation.
///
/// Native capture state is thread-local: an async operation rejects capture
/// from inside its own body and an async setup is not instrumented at all. A
/// row that captures one without an exact exclusion would either fail opaquely
/// or encode a bundle whose input surface is silently incomplete. The matrix
/// is checked here, against real discovery, before capture runs.
fn assert_capture_rows_are_synchronous(binding_key: &str, registry: &Registry, rows: &[&Row]) {
    for row in rows.iter().filter(|row| row.captures_rust()) {
        let component = row.component.as_deref().expect("component row");
        let mut asynchronous = registry
            .ops
            .iter()
            .filter(|candidate| candidate.is_async && candidate.component == component)
            .filter(|candidate| !row.capture_exclusions.iter().any(|exclusion| exclusion.operation == candidate.name))
            .map(|candidate| {
                let kind = if candidate.is_setup { "setup" } else { "operation" };
                format!("{kind} '{}'", candidate.fn_name)
            })
            .collect::<Vec<_>>();
        asynchronous.sort();
        assert!(
            asynchronous.is_empty(),
            "row '{}' captures component '{component}', which declares async {}; \
             async operations require exact captureExclusions until capture context is task-safe ({binding_key})",
            row.id,
            asynchronous.join(", ")
        );
    }
}

fn generate_csharp(root: &Path, matrix: &Matrix, out_root: &Path) {
    let rows = matrix
        .rows
        .iter()
        .filter(|row| row.classification == CLASS_COMPONENT && row.csharp.is_some())
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    let binding_key = rows[0].csharp.as_ref().expect("csharp row").binding.clone();
    assert!(
        rows.iter()
            .all(|row| row.csharp.as_ref().expect("csharp row").binding == binding_key),
        "every C# component row must use one binding"
    );
    let binding = repo_path(root, &matrix.bindings[&binding_key]);
    let binding = binding.to_str().expect("utf-8 binding path");
    let components = rows
        .iter()
        .map(|row| row.component.as_deref().expect("component row"))
        .collect::<Vec<_>>();

    let discovered = discover_many_target(binding, None, &components)
        .unwrap_or_else(|error| panic!("{binding_key}: batched C# discovery failed: {error}"));
    assert_inventory_matches_matrix(
        &binding_key,
        &components.iter().map(|component| (*component).to_string()).collect(),
        &discovered.present_components,
    );
    for row in &rows {
        let component = row.component.as_deref().expect("component row");
        let schema = discovered
            .schema(component)
            .unwrap_or_else(|| panic!("{component}: C# discovery returned no metadata"))
            .as_ref()
            .unwrap_or_else(|error| panic!("{component}: C# normalization failed: {error}"));
        let bytes = encode_registry(schema, component).unwrap_or_else(|error| panic!("{component}: C# registry encoding failed: {error}"));
        write_bytes(&row.csharp_dir(out_root).join(REGISTRY_FILE), &bytes);
    }
}

/// Fail unless the components a target actually declares are exactly the ones
/// the matrix claims. Seeding discovery from matrix rows alone would hide a
/// component that exists in a covered source file but has no row.
fn assert_inventory_matches_matrix(label: &str, expected: &BTreeSet<String>, present: &[String]) {
    let present = present.iter().cloned().collect::<BTreeSet<_>>();
    let missing = expected.difference(&present).cloned().collect::<Vec<_>>();
    let extra = present.difference(expected).cloned().collect::<Vec<_>>();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{label}: discovered components disagree with the matrix; absent from discovery: [{}]; declared by no matrix row: [{}]",
        missing.join(", "),
        extra.join(", ")
    );
}

fn error_category(message: &str) -> &'static str {
    if message.contains("operation identity must be unique") {
        "duplicate-operation-identity"
    } else if message.contains("has no operation to construct") {
        "orphan-setup"
    } else if message.contains("is a method with no receiver setup") {
        "method-missing-setup"
    } else if message.contains("is declared on private function") {
        "private-operation"
    } else if message.contains("is the dynamic runtime value") {
        "dynamic-value-type"
    } else if message.contains("has no registered component owner") || message.contains("unknown named type") {
        "unresolved-type"
    } else {
        "unclassified"
    }
}

fn normalized_error_json(id: &str, phase: &str, category: &str, component: &str, detail: &serde_json::Value) -> Vec<u8> {
    let document = serde_json::json!({
        "format": "specgate.ctsc-golden-error",
        "formatVersion": "0.1.0",
        "case": id,
        "phase": phase,
        "outcome": "failure",
        "category": category,
        "component": component,
        "detail": detail.clone(),
    });
    serde_json::to_vec(&document).expect("error document serializes")
}

fn generate_negatives(root: &Path, matrix: &Matrix, out_root: &Path, scratch: &Path) {
    let rows = matrix
        .rows
        .iter()
        .filter(|row| row.classification == CLASS_NEGATIVE)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    let discovery_rows = rows
        .iter()
        .filter(|row| row.rust.as_ref().expect("rust row").phases.iter().any(|phase| phase == "discover"))
        .collect::<Vec<_>>();
    if !discovery_rows.is_empty() {
        let binding_key = discovery_rows[0].rust.as_ref().expect("rust row").binding.clone();
        let binding = repo_path(root, &matrix.bindings[&binding_key]);
        let binding = binding.to_str().expect("utf-8 binding path");
        let components = discovery_rows
            .iter()
            .map(|row| row.component.as_deref().expect("negative component"))
            .collect::<Vec<_>>();
        let discovered = discover_many_target(binding, None, &components)
            .unwrap_or_else(|error| panic!("{binding_key}: negative discovery failed: {error}"));
        // The compile-error row is deliberately excluded: its component only
        // exists behind a Cargo feature that does not compile, so the shared
        // discovery build never links it.
        assert_inventory_matches_matrix(
            &format!("{binding_key} (discovery negatives)"),
            &components.iter().map(|component| (*component).to_string()).collect(),
            &discovered.present_components,
        );
        for row in &discovery_rows {
            let component = row.component.as_deref().expect("negative component");
            let schema = discovered
                .schema(component)
                .unwrap_or_else(|| panic!("{component}: negative discovery returned no metadata"));
            let message = match schema {
                Err(reason) => reason.clone(),
                Ok(schema) => match encode_registry(schema, component) {
                    Err(reason) => reason,
                    Ok(_bytes) => panic!(
                        "{}: expected discovery or registry encoding to reject component '{component}'",
                        row.id
                    ),
                },
            };
            let bytes = normalized_error_json(
                &row.id,
                "discover",
                error_category(&message),
                component,
                &serde_json::json!({ "message": message }),
            );
            write_bytes(&row.rust_dir(out_root).join(ERROR_FILE), &bytes);
        }
    }

    for row in rows
        .iter()
        .filter(|row| row.rust.as_ref().expect("rust row").phases.iter().any(|phase| phase == "build"))
    {
        let language = row.rust.as_ref().expect("rust row");
        let feature = language
            .feature
            .as_deref()
            .unwrap_or_else(|| panic!("{}: build negatives must name a Cargo feature", row.id));
        let binding = repo_path(root, &matrix.bindings[&language.binding]);
        let resolved = resolve_binding_target(binding.to_str().expect("utf-8 binding path"), None)
            .unwrap_or_else(|error| panic!("{}: binding resolution failed: {error}", row.id));
        let manifest = resolved.target.package_root.join("Cargo.toml");
        let target_dir = scratch.join("negative-build").join(feature);
        let output = std::process::Command::new(cargo_bin())
            .arg("build")
            .arg("--quiet")
            .arg("--message-format=json")
            .arg("--features")
            .arg(feature)
            .arg("--manifest-path")
            .arg(&manifest)
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("CARGO")
            .env_remove("CARGO_MANIFEST_DIR")
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()
            .unwrap_or_else(|error| panic!("{}: failed to invoke cargo: {error}", row.id));
        assert!(
            !output.status.success(),
            "{}: building feature '{feature}' unexpectedly succeeded",
            row.id
        );
        let intentional = intentional_sources(row);
        let rejection = intentional_compile_errors(&String::from_utf8_lossy(&output.stdout), &intentional)
            .unwrap_or_else(|error| panic!("{}: {error}", row.id));
        let bytes = normalized_error_json(
            &row.id,
            "build",
            language.expect_category.as_deref().unwrap_or("compile-error"),
            row.component.as_deref().unwrap_or_default(),
            &serde_json::json!({
                "feature": feature,
                "intentionalSources": rejection.sources,
                "diagnosticCodes": rejection.codes,
            }),
        );
        write_bytes(&row.rust_dir(out_root).join(ERROR_FILE), &bytes);
    }
}

/// The sources a row declares as the intentional fault, as repository-relative
/// forward-slash paths. Their existence is already guaranteed by the matrix
/// source-coverage check.
fn intentional_sources(row: &Row) -> BTreeSet<String> {
    assert!(
        !row.sources.is_empty(),
        "{}: a build negative must declare the source that fails to compile",
        row.id
    );
    row.sources.iter().cloned().collect()
}

/// The stable part of a compiler rejection: which intentional source it blamed
/// and which diagnostic codes it carried.
#[derive(Debug, PartialEq, Eq)]
struct CompilerRejection {
    sources: Vec<String>,
    codes: Vec<String>,
}

/// Require every compiler error from the failed build to blame only intentional
/// fixture sources.
///
/// A build that fails for any other reason — a broken dependency, a stale
/// lockfile, a renamed crate — is not the negative this row claims to cover, so
/// it is reported instead of recorded. Errors without a primary span are also
/// rejected because they cannot be attributed to the declared fault. Warnings
/// are intentionally ignored. Only the blamed source's file name and any
/// diagnostic codes are returned: rendered diagnostics carry absolute paths,
/// line numbers, and toolchain-specific wording, none of which belong in a
/// checked-in golden.
fn intentional_compile_errors(cargo_stdout: &str, intentional: &BTreeSet<String>) -> Result<CompilerRejection, String> {
    let mut sources = BTreeSet::new();
    let mut codes = BTreeSet::new();
    let mut other_sources = BTreeSet::new();
    let mut errors_without_primary_spans = 0_usize;
    let mut saw_error = false;
    for line in cargo_stdout.lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"].as_str() != Some("compiler-message") {
            continue;
        }
        let diagnostic = &message["message"];
        if diagnostic["level"].as_str() != Some("error") {
            continue;
        }
        saw_error = true;
        let primary = diagnostic["spans"]
            .as_array()
            .map(|spans| {
                spans
                    .iter()
                    .filter(|span| span["is_primary"].as_bool() == Some(true))
                    .filter_map(|span| span["file_name"].as_str())
                    .map(|file_name| file_name.replace('\\', "/"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if primary.is_empty() {
            errors_without_primary_spans += 1;
            continue;
        }
        let mut blamed_sources = BTreeSet::new();
        let mut exclusively_intentional = true;
        for file_name in primary {
            if let Some(source) = intentional.iter().find(|declared| is_source_suffix(declared.as_str(), &file_name)) {
                blamed_sources.insert(basename(source));
            } else {
                exclusively_intentional = false;
                other_sources.insert(file_name);
            }
        }
        if exclusively_intentional {
            sources.extend(blamed_sources);
            if let Some(code) = diagnostic["code"]["code"].as_str() {
                codes.insert(code.to_string());
            }
        }
    }
    if !other_sources.is_empty() || errors_without_primary_spans != 0 {
        let declared = intentional.iter().cloned().collect::<Vec<_>>().join(", ");
        let unrelated = other_sources.into_iter().collect::<Vec<_>>().join(", ");
        return Err(format!(
            "the build produced compiler error diagnostics not exclusively attributable to the intentional source(s) [{declared}]; \
             unrelated primary sources: {}; errors without primary spans: {errors_without_primary_spans}",
            if unrelated.is_empty() {
                "none".to_string()
            } else {
                format!("[{unrelated}]")
            }
        ));
    }
    if sources.is_empty() {
        let declared = intentional.iter().cloned().collect::<Vec<_>>().join(", ");
        return Err(format!(
            "the build failed without a compiler error in the intentional source(s) [{declared}]; \
             error diagnostics seen: {}",
            if saw_error {
                "none with an attributable primary span".to_string()
            } else {
                "none — the failure was not a compiler diagnostic".to_string()
            }
        ));
    }
    Ok(CompilerRejection {
        sources: sources.into_iter().collect(),
        codes: codes.into_iter().collect(),
    })
}

/// True when a compiler-reported file name identifies the declared source.
///
/// Diagnostics report paths relative to the package root, so the declared
/// repository-relative path ends with them at a separator boundary.
fn is_source_suffix(declared: &str, reported: &str) -> bool {
    let reported = reported.trim_start_matches("./");
    declared == reported || (declared.len() > reported.len() && declared.ends_with(&format!("/{reported}")))
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}
// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

fn expected_artifacts(matrix: &Matrix) -> BTreeSet<String> {
    let mut expected = BTreeSet::new();
    for row in &matrix.rows {
        for (language, name) in [(row.rust.as_ref(), "rust"), (row.csharp.as_ref(), "csharp")] {
            let Some(language) = language else { continue };
            for artifact in &language.artifacts {
                expected.insert(format!("{}/{name}/{artifact}", row.id));
            }
        }
    }
    expected
}

fn actual_artifacts(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if !root.is_dir() {
        return found;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory).unwrap_or_else(|error| panic!("failed to list {}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = relative_display(root, &path);
                if relative != MATRIX_FILE {
                    found.insert(relative);
                }
            }
        }
    }
    found
}

fn validate_generated_artifacts(matrix: &Matrix, out_root: &Path) {
    let mut problems = Vec::new();
    for row in &matrix.rows {
        for (language, name) in [(row.rust.as_ref(), "rust"), (row.csharp.as_ref(), "csharp")] {
            let Some(language) = language else { continue };
            let directory = out_root.join(&row.id).join(name);
            for artifact in &language.artifacts {
                let path = directory.join(artifact);
                if !path.is_file() {
                    problems.push(format!("{}/{name}: {artifact} was not generated", row.id));
                }
            }
            if language.expect == "failure" {
                let path = directory.join(ERROR_FILE);
                if path.is_file() {
                    let document: serde_json::Value = serde_json::from_slice(&read_bytes(&path)).expect("generated error document");
                    let category = document["category"].as_str().unwrap_or_default();
                    let expected = language.expect_category.as_deref().unwrap_or_default();
                    if category != expected {
                        problems.push(format!(
                            "{}/{name}: expected category '{expected}' but produced '{category}'",
                            row.id
                        ));
                    }
                    if document["outcome"].as_str() != Some("failure") {
                        problems.push(format!("{}/{name}: normalized error is not a failure", row.id));
                    }
                }
                continue;
            }
            if language.artifacts.iter().any(|artifact| artifact == REGISTRY_FILE) {
                let registry = directory.join(REGISTRY_FILE);
                let report = validate_registry(&registry, &[]);
                if !report.valid {
                    problems.push(format!("{}/{name}: registry validation failed: {:?}", row.id, report.issues));
                }
                if language.artifacts.iter().any(|artifact| artifact == TRACE_FILE) {
                    let trace = directory.join(TRACE_FILE);
                    let trace_report = validate_trace(&trace);
                    if !trace_report.valid {
                        problems.push(format!("{}/{name}: trace validation failed: {:?}", row.id, trace_report.issues));
                    }
                    if row.linkage_mode() != "linked" {
                        problems.push(format!(
                            "{}/{name}: captured bundles must declare linkage 'linked', found '{}'",
                            row.id,
                            row.linkage_mode()
                        ));
                    }
                    let linked = validate_linked(&trace, &registry, &[]);
                    if !linked.valid {
                        problems.push(format!("{}/{name}: linked validation failed: {:?}", row.id, linked.issues));
                    }
                    let bundle = validate_bundle(&directory);
                    if !bundle.valid {
                        problems.push(format!("{}/{name}: bundle validation failed: {:?}", row.id, bundle.issues));
                    }
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "generated CTSC artifacts failed native validation:\n  {}",
        problems.join("\n  ")
    );
}

fn check_parity(matrix: &Matrix, out_root: &Path) {
    let mut problems = Vec::new();
    for row in matrix.rows.iter().filter(|row| row.classification == CLASS_COMPONENT) {
        let rust = row.rust_dir(out_root).join(REGISTRY_FILE);
        let csharp = row.csharp_dir(out_root).join(REGISTRY_FILE);
        match row.parity.mode.as_str() {
            "byte-identical" => {
                if !csharp.is_file() {
                    problems.push(format!("{}: parity requires a C# registry", row.id));
                    continue;
                }
                if read_bytes(&rust) != read_bytes(&csharp) {
                    problems.push(format!("{}: Rust and C# registries are not byte-identical", row.id));
                }
            }
            "exception" => {
                if !csharp.is_file() {
                    problems.push(format!("{}: a parity exception requires a C# registry", row.id));
                    continue;
                }
                let rust_document: serde_json::Value = serde_json::from_slice(&read_bytes(&rust)).expect("generated Rust registry is JSON");
                let csharp_document: serde_json::Value =
                    serde_json::from_slice(&read_bytes(&csharp)).expect("generated C# registry is JSON");
                let observed = semantic_differences(&rust_document, &csharp_document);
                if observed.is_empty() {
                    problems.push(format!(
                        "{}: parity exception is stale — Rust and C# registries now match; remove the exception",
                        row.id
                    ));
                    continue;
                }
                let observed_set = observed.iter().map(ParityDifference::render).collect::<BTreeSet<_>>();
                let declared_set = row
                    .parity
                    .allowed_differences
                    .iter()
                    .map(ParityDifference::render)
                    .collect::<BTreeSet<_>>();
                for undeclared in observed_set.difference(&declared_set) {
                    problems.push(format!("{}: undeclared cross-language difference {undeclared}", row.id));
                }
                for stale in declared_set.difference(&observed_set) {
                    problems.push(format!("{}: declared cross-language difference no longer occurs {stale}", row.id));
                }
            }
            "rust-only" if row.csharp.is_some() || csharp.exists() => {
                problems.push(format!("{}: declared Rust-only but a C# artifact exists", row.id));
            }
            _ => {}
        }
    }
    assert!(problems.is_empty(), "cross-language parity failed:\n  {}", problems.join("\n  "));
}

/// Reject any generated artifact that embeds a machine-specific location.
///
/// Fixture payloads legitimately contain characters that look like Windows
/// paths, so this checks for the concrete roots a leak could come from —
/// the repository, the harness scratch tree, the home directory, and the
/// system temporary directory — rather than guessing from punctuation.
fn check_artifacts_are_portable(root: &Path, out_root: &Path, artifacts: &BTreeSet<String>) {
    let mut roots = vec![root.to_path_buf(), out_root.to_path_buf(), std::env::temp_dir()];
    for variable in [
        "SPECGATE_CTSC_GOLDENS_SCRATCH",
        "SPECGATE_CACHE_DIR",
        "USERPROFILE",
        "HOME",
        "LOCALAPPDATA",
    ] {
        if let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
            roots.push(PathBuf::from(value));
        }
    }
    let mut markers = BTreeSet::new();
    for candidate in roots {
        let display = candidate.to_string_lossy().to_string();
        if display.len() < 4 {
            continue;
        }
        markers.insert(display.replace('\\', "/"));
        markers.insert(display);
    }
    markers.insert("file://".to_string());
    markers.insert("ctsc-goldens".to_string());

    let mut problems = Vec::new();
    for artifact in artifacts {
        let path = out_root.join(artifact.replace('/', std::path::MAIN_SEPARATOR_STR));
        let text = String::from_utf8_lossy(&read_bytes(&path)).to_string();
        for marker in &markers {
            if text.contains(marker.as_str()) {
                problems.push(format!("{artifact}: leaks the machine-specific location '{marker}'"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "generated artifacts must be location independent:\n  {}",
        problems.join("\n  ")
    );
}
fn compare_against_checked_in(matrix: &Matrix, checked_in: &Path, generated: &Path) {
    let expected = expected_artifacts(matrix);
    let stored = actual_artifacts(checked_in);
    let mut problems = Vec::new();
    for missing in expected.difference(&stored) {
        problems.push(format!("missing golden: {missing}"));
    }
    for extra in stored.difference(&expected) {
        problems.push(format!("unexpected golden: {extra} (no matrix row declares it)"));
    }
    for artifact in expected.intersection(&stored) {
        let relative = artifact.replace('/', std::path::MAIN_SEPARATOR_STR);
        if read_bytes(&checked_in.join(&relative)) != read_bytes(&generated.join(&relative)) {
            problems.push(format!("stale golden: {artifact} differs from freshly generated output"));
        }
    }
    assert!(
        problems.is_empty(),
        "checked-in CTSC goldens are out of date; run `just ctsc-goldens-update`:\n  {}",
        problems.join("\n  ")
    );
}

fn verify_replay(root: &Path, matrix: &Matrix, out_root: &Path, scratch: &Path) {
    let mut by_binding: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for row in matrix
        .rows
        .iter()
        .filter(|row| row.classification == CLASS_COMPONENT && row.captures_rust())
    {
        let binding = row.rust.as_ref().expect("rust row").binding.clone();
        by_binding.entry(binding).or_default().push(row);
    }

    let mut problems = Vec::new();
    for (binding_key, rows) in by_binding {
        let binding = repo_path(root, &matrix.bindings[&binding_key]);
        let binding = binding.to_str().expect("utf-8 binding path");
        let components = rows
            .iter()
            .map(|row| row.component.as_deref().expect("component row"))
            .collect::<Vec<_>>();
        let candidates = ReplayCandidates::discover(binding, "", &components)
            .unwrap_or_else(|error| panic!("{binding_key}: candidate discovery failed: {error}"));

        let mut planned = Vec::new();
        for row in &rows {
            let plan = candidates.plan(&row.rust_dir(out_root));
            match (row.replay.mode.as_str(), plan) {
                ("verified", Ok(plan)) => {
                    let candidate = scratch.join("candidates").join(format!("{}.otlp.json", row.id.replace('/', "-")));
                    planned.push((plan, candidate, *row));
                }
                ("link-only", Ok(_plan)) => {}
                (mode @ ("verified" | "link-only"), Err(reason)) => {
                    problems.push(format!("{}: matrix marks replay '{mode}' but linking failed: {reason}", row.id));
                }
                ("unsupported", Ok(_plan)) => {
                    problems.push(format!(
                        "{}: matrix marks replay unsupported but the candidate linked successfully; update the matrix",
                        row.id
                    ));
                }
                ("unsupported", Err(reason)) => {
                    let expected = row.replay.expect_category.as_deref().expect("validated replay category");
                    match replay_failure_category(&reason) {
                        Some(actual) if actual.code() == expected => {}
                        Some(actual) => problems.push(format!(
                            "{}: expected replay failure category '{expected}' but got '{}': {reason}",
                            row.id,
                            actual.code()
                        )),
                        None => problems.push(format!(
                            "{}: replay failed for an unrelated, unclassified reason instead of '{expected}': {reason}",
                            row.id
                        )),
                    }
                }
                (mode, _) => problems.push(format!("{}: captured bundles cannot use replay mode '{mode}'", row.id)),
            }
        }
        if planned.is_empty() {
            continue;
        }
        let executable = planned
            .iter()
            .map(|(plan, candidate, _row)| (plan.clone(), candidate.clone()))
            .collect::<Vec<_>>();
        let reports = candidates
            .execute(&executable)
            .unwrap_or_else(|error| panic!("{binding_key}: batched replay failed: {error}"));
        assert_eq!(
            reports.len(),
            planned.len(),
            "{binding_key}: replay returned the wrong report count"
        );
        for (_plan, candidate, row) in &planned {
            let bundle = row.rust_dir(out_root);
            let report = compare(&bundle.join(TRACE_FILE), candidate, Some(&bundle.join(REGISTRY_FILE)), &[]);
            if !report.equivalent {
                problems.push(format!(
                    "{}: candidate replay diverged: failures={:?} mismatches={:?} errors={:?}",
                    row.id, report.validation_failures, report.mismatches, report.errors
                ));
            }
        }
    }
    assert!(problems.is_empty(), "replay verification failed:\n  {}", problems.join("\n  "));
}
// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

fn clear_generated_artifacts(root: &Path) {
    if !root.is_dir() {
        return;
    }
    let entries = std::fs::read_dir(root).unwrap_or_else(|error| panic!("failed to list {}: {error}", root.display()));
    for entry in entries {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).unwrap_or_else(|error| panic!("failed to clear {}: {error}", path.display()));
        } else if entry.file_name() != MATRIX_FILE {
            std::fs::remove_file(&path).unwrap_or_else(|error| panic!("failed to clear {}: {error}", path.display()));
        }
    }
}

fn run(mode: Mode) {
    let root = repo_root();
    let matrix = load_matrix(&root);
    check_matrix_shape(&matrix);
    check_source_coverage(&root, &matrix);
    check_replacement_references(&root, &matrix);

    let checked_in = repo_path(&root, &matrix.artifact_root);
    let scratch = std::env::var_os("SPECGATE_CTSC_GOLDENS_SCRATCH")
        .map_or_else(|| repo_path(&root, "rust/target").join("ctsc-goldens"), PathBuf::from);
    let generated = match mode {
        Mode::Update => checked_in.clone(),
        Mode::Check => scratch.join("generated"),
    };
    clear_generated_artifacts(&generated);
    std::fs::create_dir_all(&generated).unwrap_or_else(|error| panic!("failed to create {}: {error}", generated.display()));

    let binding_keys = matrix
        .rows
        .iter()
        .filter(|row| row.classification == CLASS_COMPONENT)
        .filter_map(|row| row.rust.as_ref().map(|language| language.binding.clone()))
        .collect::<BTreeSet<_>>();
    for binding_key in &binding_keys {
        generate_rust_binding(&root, &matrix, &generated, binding_key);
    }
    generate_csharp(&root, &matrix, &generated);
    generate_negatives(&root, &matrix, &generated, &scratch);

    validate_generated_artifacts(&matrix, &generated);
    check_parity(&matrix, &generated);
    let produced = actual_artifacts(&generated);
    check_artifacts_are_portable(&root, &generated, &produced);

    let expected = expected_artifacts(&matrix);
    let mut problems = Vec::new();
    for missing in expected.difference(&produced) {
        problems.push(format!("declared artifact was never generated: {missing}"));
    }
    for extra in produced.difference(&expected) {
        problems.push(format!("generated artifact is not declared by any matrix row: {extra}"));
    }
    assert!(
        problems.is_empty(),
        "the matrix and generated artifacts disagree:\n  {}",
        problems.join("\n  ")
    );

    verify_replay(&root, &matrix, &generated, &scratch);

    if mode == Mode::Check {
        compare_against_checked_in(&matrix, &checked_in, &generated);
    }

    let components = matrix.rows.iter().filter(|row| row.classification == CLASS_COMPONENT).count();
    let negatives = matrix.rows.iter().filter(|row| row.classification == CLASS_NEGATIVE).count();
    let replacements = matrix.rows.iter().filter(|row| row.classification == CLASS_REPLACEMENT).count();
    println!(
        "ctsc goldens {}: {components} component rows, {negatives} negative rows, {replacements} replacement rows, {} artifacts",
        match mode {
            Mode::Update => "updated",
            Mode::Check => "checked",
        },
        produced.len()
    );
}

/// Regenerate or verify the whole CTSC golden matrix.
///
/// Ignored by default because it drives the real Rust and .NET toolchains.
/// `just ctsc-goldens-update` and `just ctsc-goldens-check` select the mode
/// through `SPECGATE_CTSC_GOLDENS`.
#[test]
#[ignore = "drives the real Rust and .NET toolchains; run through just ctsc-goldens-check"]
fn ctsc_golden_matrix() {
    let mode = match std::env::var("SPECGATE_CTSC_GOLDENS").unwrap_or_default().as_str() {
        "update" => Mode::Update,
        "check" | "" => Mode::Check,
        other => panic!("SPECGATE_CTSC_GOLDENS must be 'update' or 'check', got '{other}'"),
    };
    run(mode);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_well_formed_and_accounts_for_every_annotated_source() {
        let root = repo_root();
        let matrix = load_matrix(&root);
        check_matrix_shape(&matrix);
        check_source_coverage(&root, &matrix);
        check_replacement_references(&root, &matrix);
    }

    #[test]
    fn negative_discovery_messages_map_to_stable_categories() {
        assert_eq!(
            error_category(
                "operation 'fixture.duplicate_identity::render' is declared 2 times; operation identity must be unique within a component"
            ),
            "duplicate-operation-identity"
        );
        assert_eq!(
            error_category(
                "setup 'make_counter' for 'fixture.missing_operation::increment' has no operation to construct; annotate the operation or remove the setup"
            ),
            "orphan-setup"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.missing_setup::increment' is a method with no receiver setup; annotate a #[spec_setup(\"increment\")] producer for its receiver"
            ),
            "method-missing-setup"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.private_operation::secret' is declared on private function 'secret'; discovery exposes only public operations"
            ),
            "private-operation"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.value::echo' output type 'value' is the dynamic runtime value; CTSC registries require declared semantic types"
            ),
            "dynamic-value-type"
        );
        assert_eq!(
            error_category("referenced type 'Gadget' has no registered component owner"),
            "unresolved-type"
        );
        assert_eq!(error_category("something else entirely"), "unclassified");
    }

    #[test]
    fn semantic_differences_are_exact_recursive_and_path_addressed() {
        let rust = serde_json::json!({
            "components": [
                { "id": "comp.app", "dependencies": [{ "componentId": "comp.core" }], "types": [] },
                { "id": "comp.core", "types": [{ "name": "Widget" }] }
            ]
        });
        let csharp = serde_json::json!({
            "components": [
                { "id": "comp.app", "types": [{ "name": "Widget" }] }
            ]
        });

        let differences = semantic_differences(&rust, &csharp);
        assert_eq!(
            differences.iter().map(|difference| difference.path.clone()).collect::<Vec<_>>(),
            vec![
                "/components".to_string(),
                "/components/0/dependencies".to_string(),
                "/components/0/types".to_string(),
            ],
            "array length first, then the common prefix element-wise, in pointer order"
        );
        assert_eq!(differences[0].rust, ParitySide::ArrayLength { length: 2 });
        assert_eq!(differences[0].csharp, ParitySide::ArrayLength { length: 1 });
        assert_eq!(differences[1].csharp, ParitySide::Missing);
        assert_eq!(differences[2].rust, ParitySide::ArrayLength { length: 0 });

        assert!(
            semantic_differences(&rust, &rust).is_empty(),
            "identical documents have no semantic difference"
        );
        assert_eq!(
            semantic_differences(&serde_json::json!({ "kind": "tuple" }), &serde_json::json!({ "kind": "record" })),
            vec![ParityDifference {
                path: "/kind".to_string(),
                rust: ParitySide::Value {
                    value: serde_json::json!("tuple")
                },
                csharp: ParitySide::Value {
                    value: serde_json::json!("record")
                },
            }]
        );
        assert_eq!(
            semantic_differences(&serde_json::json!({ "a/b": 1 }), &serde_json::json!({ "a/b": 2 }))[0].path,
            "/a~1b",
            "pointer tokens are escaped"
        );
    }

    #[test]
    fn matrix_parity_exceptions_match_the_checked_in_registries_exactly() {
        let root = repo_root();
        let matrix = load_matrix(&root);
        let artifacts = repo_path(&root, &matrix.artifact_root);
        let exceptions = matrix.rows.iter().filter(|row| row.parity.mode == "exception").collect::<Vec<_>>();
        assert!(!exceptions.is_empty(), "the matrix still declares parity exceptions");
        for row in exceptions {
            let rust: serde_json::Value =
                serde_json::from_slice(&read_bytes(&row.rust_dir(&artifacts).join(REGISTRY_FILE))).expect("Rust registry JSON");
            let csharp: serde_json::Value =
                serde_json::from_slice(&read_bytes(&row.csharp_dir(&artifacts).join(REGISTRY_FILE))).expect("C# registry JSON");
            assert_eq!(
                semantic_differences(&rust, &csharp),
                row.parity.allowed_differences,
                "{}: the declared parity exception must be the exact observed difference set",
                row.id
            );
        }
    }

    #[test]
    fn build_negatives_require_an_error_in_the_intentional_source() {
        let intentional =
            BTreeSet::from(["test/rust/negative-fixtures/specgate-ctsc-negative-fixtures/src/templates/compile_error.rs.in".to_string()]);
        let blamed = r#"{"reason":"compiler-message","message":{"level":"error","code":null,"spans":[{"is_primary":true,"file_name":"src\\templates/compile_error.rs.in"}]}}"#;
        assert_eq!(
            intentional_compile_errors(blamed, &intentional).expect("the intentional source is blamed"),
            CompilerRejection {
                sources: vec!["compile_error.rs.in".to_string()],
                codes: Vec::new(),
            }
        );

        let coded = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0425"},"spans":[{"is_primary":true,"file_name":"src/templates/compile_error.rs.in"}]}}"#;
        assert_eq!(
            intentional_compile_errors(coded, &intentional)
                .expect("codes are recorded when present")
                .codes,
            vec!["E0425".to_string()]
        );

        let unrelated = r#"{"reason":"compiler-message","message":{"level":"error","code":null,"spans":[{"is_primary":true,"file_name":"src/lib.rs"}]}}"#;
        let error = intentional_compile_errors(unrelated, &intentional).expect_err("an unrelated error is not this negative");
        assert!(error.contains("src/lib.rs"), "{error}");

        let intended_and_unrelated = format!("{coded}\n{unrelated}");
        let error = intentional_compile_errors(&intended_and_unrelated, &intentional)
            .expect_err("one intended error may not hide an unrelated compiler error");
        assert!(error.contains("not exclusively attributable"), "{error}");
        assert!(error.contains("src/lib.rs"), "{error}");

        let unspanned = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0999"},"spans":[]}}"#;
        let intended_and_unspanned = format!("{coded}\n{unspanned}");
        let error = intentional_compile_errors(&intended_and_unspanned, &intentional)
            .expect_err("an unspanned compiler error cannot be attributed to the intentional source");
        assert!(error.contains("errors without primary spans: 1"), "{error}");

        let warning_only = r#"{"reason":"compiler-message","message":{"level":"warning","code":null,"spans":[{"is_primary":true,"file_name":"src/templates/compile_error.rs.in"}]}}"#;
        let error = intentional_compile_errors(warning_only, &intentional).expect_err("a warning is not a rejection");
        assert!(error.contains("not a compiler diagnostic"), "{error}");
        assert!(
            intentional_compile_errors("", &intentional).is_err(),
            "a build that produced no diagnostics at all cannot be this negative"
        );
    }

    #[test]
    fn matrix_inventories_reject_missing_and_extra_components() {
        let expected = BTreeSet::from(["fixture.one".to_string(), "fixture.two".to_string()]);
        assert_inventory_matches_matrix("label", &expected, &["fixture.one".to_string(), "fixture.two".to_string()]);

        let missing = std::panic::catch_unwind(|| {
            assert_inventory_matches_matrix("label", &expected, &["fixture.one".to_string()]);
        })
        .expect_err("a component the matrix declares but discovery lacks must fail");
        assert!(panic_message(missing.as_ref()).contains("absent from discovery: [fixture.two]"));

        let extra = std::panic::catch_unwind(|| {
            assert_inventory_matches_matrix(
                "label",
                &expected,
                &["fixture.one".to_string(), "fixture.two".to_string(), "fixture.three".to_string()],
            );
        })
        .expect_err("a compiled component no row declares must fail");
        assert!(panic_message(extra.as_ref()).contains("declared by no matrix row: [fixture.three]"));
    }

    #[test]
    fn capture_coverage_rejects_one_untested_operation_out_of_two() {
        let operation = |name: &str| ("fixture.two_operations".to_string(), name.to_string());
        let discovered = BTreeSet::from([operation("outer"), operation("inner")]);
        let captured = BTreeSet::from([operation("outer")]);
        let problems = capture_coverage_problems("component/fixture.two_operations", &discovered, &captured, &BTreeSet::new());
        assert_eq!(
            problems,
            vec![
                "component/fixture.two_operations: discovered operations have no captured trace: \
                 [fixture.two_operations::inner]"
                    .to_string()
            ]
        );

        let nested_capture = BTreeSet::from([operation("outer"), operation("inner")]);
        assert!(
            capture_coverage_problems("component/fixture.two_operations", &discovered, &nested_capture, &BTreeSet::new()).is_empty(),
            "a nested operation span exercises the semantic operation"
        );

        let excluded = BTreeSet::from([operation("inner")]);
        assert!(
            capture_coverage_problems("component/fixture.two_operations", &discovered, &captured, &excluded).is_empty(),
            "an exact operation-level exclusion covers the one intentional gap"
        );
        assert!(
            capture_coverage_problems("component/fixture.two_operations", &discovered, &nested_capture, &excluded)[0]
                .contains("capture exclusions are stale"),
            "an exclusion fails once the operation is captured"
        );
    }

    fn synthetic_component_row(component: &str, phases: &[&str], limitations: &serde_json::Value) -> Row {
        let capturing = phases.contains(&"capture");
        let artifacts = if capturing {
            serde_json::json!([MANIFEST_FILE, TRACE_FILE, REGISTRY_FILE])
        } else {
            serde_json::json!([REGISTRY_FILE])
        };
        let replay = if capturing {
            serde_json::json!({ "mode": "verified" })
        } else {
            serde_json::json!({ "mode": "not-applicable", "reason": "Discovery-only rows own no bundle." })
        };
        let linkage = if capturing {
            serde_json::json!({ "mode": "linked" })
        } else {
            serde_json::Value::Null
        };
        serde_json::from_value(serde_json::json!({
            "id": format!("component/{component}"),
            "classification": CLASS_COMPONENT,
            "sourceGroup": "synthetic",
            "component": component,
            "sources": ["test/rust/crates/specgate-ctsc-fixtures/src/synthetic.rs"],
            "rust": {
                "binding": "rust",
                "phases": phases,
                "expect": "success",
                "artifacts": artifacts,
            },
            "parity": { "mode": "rust-only", "reason": "Synthetic row used only by this test." },
            "replay": replay,
            "linkage": linkage,
            "limitations": limitations,
        }))
        .expect("synthetic row deserializes")
    }

    fn synthetic_async_registry() -> Registry {
        Registry::parse(
            r#"{"operations":[
                {"name":"fetch","module_path":"fixture","fn_name":"fetch","is_setup":false,"is_async":true,"is_method":false,"is_public":true,"return_type":"String","fills":"","params":[],"component":"fixture.async_operation"},
                {"name":"advance","module_path":"fixture","fn_name":"advance","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.async_setup"},
                {"name":"advance","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":true,"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[],"component":"fixture.async_setup"},
                {"name":"add","module_path":"fixture","fn_name":"add","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"i32","fills":"","params":[],"component":"fixture.sync"}
            ],"types":[]}"#,
        )
        .expect("synthetic registry parses")
    }

    /// Capture cannot instrument async work in either direction: an async
    /// operation rejects capture from its own body and an async setup records
    /// nothing at all. A row that claims a capture bundle for such a component
    /// must fail here, naming the async declaration.
    #[test]
    fn capture_rows_may_not_declare_async_operations_or_setups() {
        let registry = synthetic_async_registry();
        let unlimited = serde_json::json!([]);
        let synchronous = synthetic_component_row("fixture.sync", &["discover", "capture"], &unlimited);
        assert_capture_rows_are_synchronous("rust", &registry, &[&synchronous]);

        let limitation = serde_json::json!([{ "code": ASYNC_LIMITATION, "detail": "Capture context is not task-safe." }]);
        for (component, expected) in [
            ("fixture.async_operation", "operation 'fetch'"),
            ("fixture.async_setup", "setup 'make'"),
        ] {
            let discovery_only = synthetic_component_row(component, &["discover"], &limitation);
            assert_capture_rows_are_synchronous("rust", &registry, &[&discovery_only]);

            let capturing = synthetic_component_row(component, &["discover", "capture"], &unlimited);
            let failure = std::panic::catch_unwind(|| {
                assert_capture_rows_are_synchronous("rust", &registry, &[&capturing]);
            })
            .expect_err("a capture row over an async component must fail");
            let message = panic_message(failure.as_ref());
            assert!(message.contains(expected), "{message}");
            assert!(message.contains("captureExclusions"), "{message}");

            let mut excluded = synthetic_component_row(component, &["discover", "capture"], &unlimited);
            excluded.capture_exclusions.push(CaptureExclusion {
                operation: if component == "fixture.async_operation" {
                    "fetch".to_string()
                } else {
                    "advance".to_string()
                },
                code: ASYNC_LIMITATION.to_string(),
                reason: "Native capture context is not task-safe.".to_string(),
            });
            assert_capture_rows_are_synchronous("rust", &registry, &[&excluded]);
        }
    }

    /// The limitation is a claim about the row, not a comment: declaring it
    /// while still capturing would document a restriction the row violates.
    #[test]
    fn the_async_limitation_forces_a_discovery_only_row() {
        let limitation = serde_json::json!([{ "code": ASYNC_LIMITATION, "detail": "Capture context is not task-safe." }]);
        let matrix = |row: Row| Matrix {
            format: "specgate.ctsc-golden-matrix".to_string(),
            format_version: "0.1.0".to_string(),
            artifact_root: "test/goldens/ctsc".to_string(),
            bindings: BTreeMap::from([("rust".to_string(), "test/bindings/rust.yaml".to_string())]),
            sources: SourceCoverage {
                roots: Vec::new(),
                exclusions: Vec::new(),
            },
            rows: vec![row],
        };

        check_matrix_shape(&matrix(synthetic_component_row("fixture.async_setup", &["discover"], &limitation)));

        let capturing = matrix(synthetic_component_row(
            "fixture.async_setup",
            &["discover", "capture"],
            &limitation,
        ));
        let failure = std::panic::catch_unwind(|| check_matrix_shape(&capturing))
            .expect_err("an async-limited row that captures must fail matrix shape");
        assert!(panic_message(failure.as_ref()).contains("must stay discovery-only"));
    }

    fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_string()))
            .unwrap_or_default()
    }

    #[test]
    fn normalized_errors_carry_only_stable_semantic_detail() {
        let bytes = normalized_error_json(
            "negative/private-operation",
            "discover",
            "private-operation",
            "fixture.private_operation",
            &serde_json::json!({ "message": "operation 'fixture.private_operation::secret' is not public" }),
        );
        let text = String::from_utf8(bytes).expect("utf-8 error document");
        assert!(!text.contains('\n'), "normalized errors must be compact");
        assert!(
            !text.contains(":\\") && !text.contains("file://"),
            "normalized errors must not carry paths"
        );
        let document: serde_json::Value = serde_json::from_str(&text).expect("valid error document");
        assert_eq!(document["format"], "specgate.ctsc-golden-error");
        assert_eq!(document["phase"], "discover");
        assert_eq!(document["outcome"], "failure");
        assert_eq!(document["category"], "private-operation");
        assert_eq!(document["component"], "fixture.private_operation");
    }
}
