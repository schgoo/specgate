#![cfg(any(test, feature = "test-util"))]

//! Negative-golden diagnostic classification and invariant checks.
//!
//! Discovery messages are mapped to stable categories so expected failures can
//! be compared without weakening the exact generated golden workflow.
use super::*;

const DUPLICATE_OPERATION: &str = "operation identity must be unique";
const ORPHAN_SETUP: &str = "has no operation to construct";
const MISSING_RECEIVER: &str = "is a method with no receiver setup";
const PRIVATE_OPERATION: &str = "is declared on private function";
const DYNAMIC_VALUE: &str = "is the dynamic runtime value";
const MISSING_OWNER: &str = "has no registered component owner";
const UNKNOWN_TYPE: &str = "unknown named type";

/// Classify a normalized discovery diagnostic into a stable golden category.
pub(super) fn error_category(message: impl AsRef<str>) -> &'static str {
    let message = message.as_ref();
    // These fragments are the stable diagnostic contracts emitted by
    // discovery. Changing their wording requires coordinated golden-category
    // updates or affected failures become deliberately "unclassified".
    if message.contains(DUPLICATE_OPERATION) {
        "duplicate-operation-identity"
    } else if message.contains(ORPHAN_SETUP) {
        "orphan-setup"
    } else if message.contains(MISSING_RECEIVER) {
        "method-missing-setup"
    } else if message.contains(PRIVATE_OPERATION) {
        "private-operation"
    } else if message.contains(DYNAMIC_VALUE) {
        "dynamic-value-type"
    } else if message.contains(MISSING_OWNER) || message.contains(UNKNOWN_TYPE) {
        "unresolved-type"
    } else {
        "unclassified"
    }
}

/// Normalized stable fields for one generated negative-case artifact.
#[derive(Clone, Copy)]
pub(super) struct ErrorInput<'a> {
    pub(super) id: &'a str,
    pub(super) phase: Phase,
    pub(super) category: &'a str,
    pub(super) component: &'a str,
    pub(super) detail: &'a serde_json::Value,
}

/// Encode one compact, path-free negative-case artifact.
pub(super) fn error_json(input: ErrorInput<'_>) -> Vec<u8> {
    let document = serde_json::json!({
        "format": ERROR_FORMAT,
        "formatVersion": ERROR_VERSION,
        "case": input.id,
        "phase": input.phase,
        "outcome": "failure",
        "category": input.category,
        "component": input.component,
        "detail": input.detail,
    });
    serde_json::to_vec(&document).expect("error document serializes")
}

/// Generate every discovery and build negative declared by the matrix.
pub(super) fn generate_negatives(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    scratch: impl AsRef<Path>,
) -> Result<(), GoldenError> {
    generate_with(root, matrix, out_root, scratch, cargo_build)
}

fn rust_language(row: &Row) -> Result<&LanguageRow, GoldenError> {
    row.rust
        .as_ref()
        .ok_or_else(|| GoldenError::message(format!("{}: negative row must define Rust settings", row.id)))
}

fn negative_component(row: &Row) -> Result<&str, GoldenError> {
    row.component
        .as_deref()
        .ok_or_else(|| GoldenError::message(format!("{}: discovery-negative row must name a component", row.id)))
}

fn generate_with(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    scratch: impl AsRef<Path>,
    build: impl Fn(&Path, &Path, &str) -> std::io::Result<std::process::Output>,
) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let out_root = out_root.as_ref();
    let scratch = scratch.as_ref();
    let rows = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::NegativeFixture)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(());
    }
    let mut discovery_rows = Vec::new();
    for row in &rows {
        if rust_language(row)?.phases.contains(&Phase::Discover) {
            discovery_rows.push(*row);
        }
    }
    if !discovery_rows.is_empty() {
        let binding_key = rust_language(discovery_rows[0])?.binding.clone();
        let binding = repo_path(root, &matrix.bindings[&binding_key]);
        let binding = binding
            .to_str()
            .ok_or_else(|| GoldenError::message(format!("{binding_key}: binding path is not UTF-8")))?;
        let components = discovery_rows
            .iter()
            .map(|row| negative_component(row))
            .collect::<Result<Vec<_>, _>>()?;
        let component_ids = components
            .iter()
            .copied()
            .map(specgate_discovery::identity::ComponentId::from)
            .collect::<Vec<_>>();
        let discovered = discover_batch(binding, None, &component_ids)
            .map_err(|source| GoldenError::wrap(format!("{binding_key}: negative discovery failed"), source))?;
        // The compile-error row is deliberately excluded: its component only
        // exists behind a Cargo feature that does not compile, so the shared
        // discovery build never links it.
        if let Some(diagnostic) = inventory_mismatch(
            format!("{binding_key} (discovery negatives)"),
            &components.iter().map(|component| (*component).to_string()).collect(),
            &discovered.present_components,
        ) {
            return Err(GoldenError::message(diagnostic));
        }
        for row in &discovery_rows {
            let component = negative_component(row)?;
            let message = match discovered.schema(component) {
                SchemaLookup::Invalid(reason) => reason.to_string(),
                SchemaLookup::Found(schema) => match registry_bytes(schema, component) {
                    Err(reason) => reason.to_string(),
                    Ok(_bytes) => {
                        return Err(GoldenError::message(format!(
                            "{}: expected discovery or registry encoding to reject component '{component}'",
                            row.id
                        )));
                    }
                },
                SchemaLookup::Missing => {
                    return Err(GoldenError::message(format!(
                        "{}: negative discovery returned no metadata for '{component}'",
                        row.id
                    )));
                }
            };
            let bytes = error_json(ErrorInput {
                id: &row.id,
                phase: Phase::Discover,
                category: error_category(&message),
                component,
                detail: &serde_json::json!({ "message": message }),
            });
            write_bytes(row.rust_dir(out_root).join(error_file()), &bytes)?;
        }
    }

    for row in rows {
        let language = rust_language(row)?;
        if !language.phases.contains(&Phase::Build) {
            continue;
        }
        let feature = language
            .feature
            .as_deref()
            .ok_or_else(|| GoldenError::message(format!("{}: build-negative row must name a Cargo feature", row.id)))?;
        let binding = repo_path(root, &matrix.bindings[&language.binding]);
        let binding_text = binding
            .to_str()
            .ok_or_else(|| GoldenError::message(format!("{}: binding path is not UTF-8", row.id)))?;
        let resolved = resolve_target(binding_text, None)
            .map_err(|source| GoldenError::wrap(format!("{}: binding resolution failed", row.id), source))?;
        let manifest = resolved.target.package_root.join("Cargo.toml");
        let target_dir = scratch.join("negative-build").join(feature);
        let output = build(&manifest, &target_dir, feature)
            .map_err(|source| GoldenError::wrap(format!("{}: failed to invoke cargo", row.id), source))?;
        if output.status.success() {
            return Err(GoldenError::message(format!(
                "{}: building feature '{feature}' unexpectedly succeeded",
                row.id
            )));
        }
        let intentional = intentional_sources(row);
        let rejection = compile_errors(String::from_utf8_lossy(&output.stdout), &intentional)
            .map_err(|source| GoldenError::wrap(format!("{}: compiler diagnostic attribution failed", row.id), source))?;
        let bytes = error_json(ErrorInput {
            id: &row.id,
            phase: Phase::Build,
            category: language.expect_category.as_deref().unwrap_or("compile-error"),
            component: row.component.as_deref().unwrap_or_default(),
            detail: &serde_json::json!({
                "feature": feature,
                "intentionalSources": rejection.sources,
                "diagnosticCodes": rejection.codes,
            }),
        });
        write_bytes(row.rust_dir(out_root).join(error_file()), &bytes)?;
    }
    Ok(())
}

fn cargo_build(manifest: &Path, target_dir: &Path, feature: &str) -> std::io::Result<std::process::Output> {
    std::process::Command::new(cargo_bin())
        .arg("build")
        .arg("--quiet")
        .arg("--message-format=json")
        .arg("--features")
        .arg(feature)
        .arg("--manifest-path")
        .arg(manifest)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
}

/// The sources a row declares as the intentional fault, as repository-relative
/// forward-slash paths. Their existence is already guaranteed by the matrix
/// source-coverage check.
pub(super) fn intentional_sources(row: &Row) -> BTreeSet<RepoPath> {
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
pub(super) struct CompilerRejection {
    pub(super) sources: Vec<RepoPath>,
    pub(super) codes: Vec<String>,
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
pub(super) fn compile_errors(cargo_stdout: impl AsRef<str>, intentional: &BTreeSet<RepoPath>) -> Result<CompilerRejection, CompileError> {
    let cargo_stdout = cargo_stdout.as_ref();
    let mut sources = BTreeSet::new();
    let mut codes = BTreeSet::new();
    let mut other_sources = BTreeSet::new();
    let mut unspanned_errors = 0_usize;
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
                    .map(|file_name| PathBuf::from(file_name.replace('\\', "/")))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if primary.is_empty() {
            unspanned_errors += 1;
            continue;
        }
        let mut blamed_sources = BTreeSet::new();
        let mut exclusively_intentional = true;
        for file_name in primary {
            if let Some(source) = intentional.iter().find(|declared| source_matches(declared, &file_name)) {
                blamed_sources.insert(
                    RepoPath::try_from(
                        basename(source)
                            .to_str()
                            .expect("declared repository source basename must remain UTF-8"),
                    )
                    .expect("compiler source basename must be a portable relative path"),
                );
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
    if !other_sources.is_empty() || unspanned_errors != 0 {
        let declared_sources = intentional.iter().map(|source| source.as_str().to_owned()).collect::<Vec<_>>();
        let unrelated_sources = other_sources.into_iter().collect::<Vec<_>>();
        let unrelated_display = unrelated_sources.iter().map(|path| path.display().to_string()).collect::<Vec<_>>();
        let diagnostic = format!(
            "the build produced compiler error diagnostics not exclusively attributable to the intentional source(s) [{}]; unrelated primary sources: {}; errors without primary spans: {unspanned_errors}",
            declared_sources.join(", "),
            if unrelated_display.is_empty() {
                "none".to_string()
            } else {
                format!("[{}]", unrelated_display.join(", "))
            },
        );
        return Err(CompileError::attribution(
            diagnostic,
            Attribution::new(declared_sources.into_iter().map(Into::into), unrelated_sources, unspanned_errors),
        ));
    }
    if sources.is_empty() {
        let declared_sources = intentional.iter().map(|source| source.as_str().to_owned()).collect::<Vec<_>>();
        let diagnostic = format!(
            "the build failed without a compiler error in the intentional source(s) [{}]; error diagnostics seen: {}",
            declared_sources.join(", "),
            if saw_error {
                "none with an attributable primary span"
            } else {
                "none — the failure was not a compiler diagnostic"
            },
        );
        return Err(CompileError::attribution(
            diagnostic,
            Attribution::new(declared_sources.into_iter().map(Into::into), Vec::<PathBuf>::new(), 0),
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
pub(super) fn source_matches(declared: &RepoPath, reported: impl AsRef<Path>) -> bool {
    let declared: &Path = declared.as_ref();
    let mut reported = reported.as_ref();
    while let Ok(remainder) = reported.strip_prefix(".") {
        reported = remainder;
    }
    declared == reported || declared.ends_with(reported)
}

/// Return the terminal filename from a compiler-reported source path.
pub(super) fn basename(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    path.file_name().map_or_else(|| path.to_path_buf(), PathBuf::from)
}
// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------
