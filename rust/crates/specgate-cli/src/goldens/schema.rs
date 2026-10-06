#![cfg(any(test, feature = "test-util"))]

//! Typed schema and semantic parity model for the checked-in golden matrix.
use std::fmt::Write as _;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Update,
    Check,
}

/// Invalid portable repository-relative path.
#[ohno::error]
#[display("'{value}' is not a portable repository-relative path\nbacktrace:\n{backtrace}")]
pub(super) struct PathError {
    value: String,
    backtrace: std::backtrace::Backtrace,
}

/// Validated portable repository-relative path from the matrix wire format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct RepoPath(String);

impl RepoPath {
    fn parse(value: impl Into<String>) -> Result<Self, PathError> {
        let value = value.into();
        let windows_prefix = value
            .as_bytes()
            .get(0..2)
            .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':');
        let invalid = value.is_empty()
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains('\\')
            || windows_prefix
            || value
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..");
        if invalid {
            Err(PathError::new(value, std::backtrace::Backtrace::capture()))
        } else {
            Ok(Self(value))
        }
    }
}
impl Serialize for RepoPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for RepoPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl AsRef<Path> for RepoPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}
impl RepoPath {
    /// Borrow the validated repository-relative forward-slash spelling.
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert an operating-system path to the validated matrix path spelling.
    ///
    /// Returns a path error when the normalized path is absolute or traverses upward.
    pub(super) fn from_path(path: impl AsRef<Path>) -> Result<Self, PathError> {
        Self::parse(path.as_ref().to_string_lossy().replace('\\', "/"))
    }
}
impl TryFrom<&str> for RepoPath {
    type Error = PathError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}
impl TryFrom<String> for RepoPath {
    type Error = PathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}
impl std::borrow::Borrow<str> for RepoPath {
    fn borrow(&self) -> &str {
        &self.0
    }
}
impl std::ops::Deref for RepoPath {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::fmt::Display for RepoPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Matrix row purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Classification {
    ImplementationComponent,
    NegativeFixture,
    Replacement,
}

/// Pipeline phase exercised by a language row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Phase {
    Discover,
    Capture,
    Build,
}

/// Expected phase outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Expectation {
    Success,
    Failure,
}

/// Cross-language comparison policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ParityMode {
    ByteIdentical,
    Exception,
    RustOnly,
    NotApplicable,
}

/// Candidate replay policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ReplayMode {
    Verified,
    LinkOnly,
    Unsupported,
    NotApplicable,
}

/// Capture-to-registry linkage policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum LinkageMode {
    Linked,
    NotApplicable,
}

/// Complete hand-authored matrix document and artifact inventory.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Matrix {
    /// Matrix format discriminator.
    pub(super) format: String,
    /// Matrix schema version.
    pub(super) format_version: String,
    /// Checked-in generated artifact root.
    pub(super) artifact_root: RepoPath,
    /// Language binding paths keyed by matrix name.
    pub(super) bindings: BTreeMap<String, RepoPath>,
    /// Source coverage policy.
    pub(super) sources: SourceCoverage,
    /// Ordered conformance rows.
    pub(super) rows: Vec<Row>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// Source roots and explicit coverage exclusions.
pub(super) struct SourceCoverage {
    /// Repository roots scanned for annotations.
    pub(super) roots: Vec<RepoPath>,
    /// Explicitly justified uncovered paths.
    pub(super) exclusions: Vec<Exclusion>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// One documented source-coverage exclusion.
pub(super) struct Exclusion {
    /// Stable exclusion rule identifier.
    pub(super) rule: String,
    /// Human-readable exclusion rationale.
    pub(super) description: String,
    #[serde(default)]
    /// Paths governed by this exclusion.
    pub(super) paths: Vec<RepoPath>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// One conformance fixture, negative case, or replacement row.
pub(super) struct Row {
    /// Stable row identifier and artifact-relative path.
    pub(super) id: RepoPath,
    /// Row purpose.
    pub(super) classification: Classification,
    /// Review grouping for related fixtures.
    pub(super) source_group: String,
    #[serde(default)]
    /// Optional CTSC component identity.
    pub(super) component: Option<String>,
    #[serde(default)]
    /// Annotated source files covered by the row.
    pub(super) sources: Vec<RepoPath>,
    #[serde(default)]
    /// Rust expectations when applicable.
    pub(super) rust: Option<LanguageRow>,
    #[serde(default)]
    /// C# expectations when applicable.
    pub(super) csharp: Option<LanguageRow>,
    /// Cross-language parity expectation.
    pub(super) parity: Parity,
    /// Replay expectation.
    pub(super) replay: Replay,
    #[serde(default)]
    /// Capture linkage expectation when a bundle exists.
    pub(super) linkage: Option<Linkage>,
    #[serde(default)]
    /// Explicit limitations demonstrated by the row.
    pub(super) limitations: Vec<Limitation>,
    #[serde(default)]
    /// Operations intentionally omitted from capture.
    pub(super) capture_exclusions: Vec<CaptureExclusion>,
    #[serde(default)]
    /// Native replacement evidence, when applicable.
    pub(super) replacement: Option<Replacement>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// Expected phases and artifacts for one language binding.
pub(super) struct LanguageRow {
    /// Binding key from the matrix binding map.
    pub(super) binding: String,
    /// Required pipeline phases.
    pub(super) phases: Vec<Phase>,
    /// Expected success or failure.
    pub(super) expect: Expectation,
    /// Exact artifact filenames owned by this language row.
    pub(super) artifacts: Vec<RepoPath>,
    #[serde(default)]
    /// Stable expected failure category.
    pub(super) expect_category: Option<String>,
    #[serde(default)]
    /// Cargo feature enabling a build-negative fixture.
    pub(super) feature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// Cross-language registry parity policy.
pub(super) struct Parity {
    /// Required parity comparison mode.
    pub(super) mode: ParityMode,
    #[serde(default)]
    /// Required rationale for non-default behavior.
    pub(super) reason: Option<String>,
    /// Every semantic difference a `exception` row is allowed to have, as an
    /// exact machine-readable set. The observed Rust-vs-C# diff must equal this
    /// set, so a new, missing, or changed difference fails the gate.
    #[serde(default)]
    pub(super) allowed_differences: Vec<ParityDifference>,
}

/// One declared or observed semantic difference between the Rust and C#
/// registry documents of a component.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ParityDifference {
    /// JSON Pointer into the registry document.
    pub(super) path: String,
    /// Rust-side observed value.
    pub(super) rust: ParitySide,
    /// C#-side observed value.
    pub(super) csharp: ParitySide,
}

/// One side of a semantic difference.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "camelCase")]
pub(super) enum ParitySide {
    /// The language does not emit this member at all.
    Missing,
    /// Both languages emit an array here, with different lengths.
    ArrayLength { length: usize },
    /// The language emits this exact value.
    Value { value: serde_json::Value },
}

impl ParityDifference {
    /// Render the stable JSON diagnostic compared by parity golden tests.
    pub(super) fn render(&self) -> String {
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
pub(super) fn semantic_differences(rust: &serde_json::Value, csharp: &serde_json::Value) -> Vec<ParityDifference> {
    let mut differences = Vec::new();
    collect_differences(rust, csharp, "", &mut differences);
    differences.shrink_to_fit();
    differences
}

/// Append semantic differences beneath an existing JSON Pointer.
///
/// Existing output entries are retained. Object keys are traversed in lexical
/// order and array indexes in wire order, giving callers deterministic output.
pub(super) fn collect_differences(
    rust: &serde_json::Value,
    csharp: &serde_json::Value,
    path: impl AsRef<str>,
    out: &mut Vec<ParityDifference>,
) {
    let mut path = path.as_ref().to_owned();
    collect_at(rust, csharp, &mut path, out);
}

fn collect_at(rust: &serde_json::Value, csharp: &serde_json::Value, path: &mut String, out: &mut Vec<ParityDifference>) {
    if rust == csharp {
        return;
    }
    match (rust, csharp) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            let names = left.keys().chain(right.keys()).cloned().collect::<BTreeSet<_>>();
            for name in names {
                let original_len = path.len();
                path.push('/');
                for character in name.chars() {
                    match character {
                        '~' => path.push_str("~0"),
                        '/' => path.push_str("~1"),
                        other => path.push(other),
                    }
                }
                match (left.get(&name), right.get(&name)) {
                    (Some(left_value), Some(right_value)) => collect_at(left_value, right_value, path, out),
                    (Some(left_value), None) => out.push(ParityDifference {
                        path: path.clone(),
                        rust: ParitySide::Value { value: left_value.clone() },
                        csharp: ParitySide::Missing,
                    }),
                    (None, Some(right_value)) => out.push(ParityDifference {
                        path: path.clone(),
                        rust: ParitySide::Missing,
                        csharp: ParitySide::Value {
                            value: right_value.clone(),
                        },
                    }),
                    (None, None) => unreachable!("name came from one of the two objects"),
                }
                path.truncate(original_len);
            }
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            if left.len() != right.len() {
                out.push(ParityDifference {
                    path: path.clone(),
                    rust: ParitySide::ArrayLength { length: left.len() },
                    csharp: ParitySide::ArrayLength { length: right.len() },
                });
            }
            for (index, (left_value, right_value)) in left.iter().zip(right.iter()).enumerate() {
                let original_len = path.len();
                write!(path, "/{index}").expect("writing to a String cannot fail");
                collect_at(left_value, right_value, path, out);
                path.truncate(original_len);
            }
        }
        _ => out.push(ParityDifference {
            path: path.clone(),
            rust: ParitySide::Value { value: rust.clone() },
            csharp: ParitySide::Value { value: csharp.clone() },
        }),
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// Candidate replay expectation for one row.
pub(super) struct Replay {
    /// Replay support mode.
    pub(super) mode: ReplayMode,
    #[serde(default)]
    /// Required rationale for non-default behavior.
    pub(super) reason: Option<String>,
    #[serde(default)]
    /// Stable expected failure category.
    pub(super) expect_category: Option<String>,
}

/// How a captured bundle relates to its own registry.
///
/// Every captured Rust bundle is `linked`: it passes native trace, Linked, and
/// bundle validation against the registry discovered from the same sources.
/// `not-applicable` belongs to discovery-only rows, which own no bundle. There
/// is no exception mode — a bundle that cannot link is a product defect.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Linkage {
    /// Required linkage mode.
    pub(super) mode: LinkageMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// One declared product limitation attached to a row.
pub(super) struct Limitation {
    /// Stable machine-readable code.
    pub(super) code: String,
    /// Human-readable limitation detail.
    pub(super) detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// One operation intentionally excluded from native capture.
pub(super) struct CaptureExclusion {
    /// Operation omitted from capture.
    pub(super) operation: String,
    /// Stable machine-readable code.
    pub(super) code: String,
    /// Human-readable rationale.
    pub(super) reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// Existing native test coverage replacing generated artifacts.
pub(super) struct Replacement {
    /// Native feature or coverage area.
    pub(super) feature: String,
    /// Why native coverage replaces generated artifacts.
    pub(super) rationale: String,
    /// Existing test references proving coverage.
    pub(super) references: Vec<Reference>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
/// Repository test item referenced by a replacement row.
pub(super) struct Reference {
    /// Repository source path.
    pub(super) path: RepoPath,
    /// Test item name declared by the source.
    pub(super) test: String,
}

impl Row {
    /// Resolve this row's Rust artifact directory below the supplied root.
    pub(super) fn rust_dir(&self, root: impl AsRef<Path>) -> PathBuf {
        root.as_ref().join(&self.id).join("rust")
    }

    /// Resolve this row's C# artifact directory below the supplied root.
    pub(super) fn csharp_dir(&self, root: impl AsRef<Path>) -> PathBuf {
        root.as_ref().join(&self.id).join("csharp")
    }

    /// Return the declared linkage mode, or not-applicable when omitted.
    pub(super) fn linkage_mode(&self) -> LinkageMode {
        self.linkage.as_ref().map_or(LinkageMode::NotApplicable, |linkage| linkage.mode)
    }

    /// Whether the Rust side of this row participates in capture.
    pub(super) fn captures_rust(&self) -> bool {
        self.rust.as_ref().is_some_and(|language| language.phases.contains(&Phase::Capture))
    }
}
