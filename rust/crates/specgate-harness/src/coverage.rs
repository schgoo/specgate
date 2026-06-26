//! Code-coverage measurement for spec runs.
//!
//! When coverage is enabled, each target group's runner is built and executed
//! with `-C instrument-coverage`, writing `.profraw` profiles. After all groups
//! run, the profiles are merged with `llvm-profdata` and exported with
//! `llvm-cov`, filtered to the implementation source files the spec exercised,
//! to produce a [`CoverageReport`].
//!
//! The `llvm-profdata`/`llvm-cov` tools ship with the `llvm-tools` rustup
//! component; if absent, coverage degrades gracefully to "unavailable".

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::{CoverageReport, FileCoverage};

/// Collect every implementation `.rs` file of the crate at `package_root` (the
/// "crate under test"). Scans `package_root/src` when present, else the root;
/// skips `target/` build artifacts and `tests/` (test code is not the subject).
pub(crate) fn collect_crate_sources(package_root: &Path) -> Vec<PathBuf> {
    let start = {
        let src = package_root.join("src");
        if src.is_dir() { src } else { package_root.to_path_buf() }
    };
    let mut out = Vec::new();
    let mut stack = vec![start];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "target" || name == "tests" {
                    continue;
                }
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(std::fs::canonicalize(&p).unwrap_or(p));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Coverage artifacts captured from one target group's instrumented run.
pub(crate) struct GroupCoverage {
    /// The compiled runner binary (its instrumented code links the impl crate).
    pub binary: PathBuf,
    /// Directory containing the `.profraw` files the run produced.
    pub profraw_dir: PathBuf,
    /// Implementation source files to scope the report to.
    pub sources: Vec<PathBuf>,
}

/// Returns the env vars to set on an instrumented `cargo run`, plus the profraw
/// directory they write to. Caller passes the vars to the build/run command.
pub(crate) fn instrumentation_env(scratch_dir: &Path) -> (Vec<(String, String)>, PathBuf) {
    let profraw_dir = scratch_dir.join("coverage");
    let _ = std::fs::create_dir_all(&profraw_dir);
    // %p = pid, %m = binary signature — keeps profraw files distinct.
    let profile_pattern = profraw_dir.join("run-%p-%m.profraw");
    let env = vec![
        ("RUSTFLAGS".to_string(), "-C instrument-coverage".to_string()),
        ("LLVM_PROFILE_FILE".to_string(), profile_pattern.to_string_lossy().into_owned()),
    ];
    (env, profraw_dir)
}

/// Locate an `llvm-*` tool from the active toolchain's `llvm-tools` component.
/// Returns `None` if the component is not installed.
fn llvm_tool(name: &str) -> Option<PathBuf> {
    let sysroot = Command::new("rustc").arg("--print").arg("sysroot").output().ok()?;
    if !sysroot.status.success() {
        return None;
    }
    let sysroot = String::from_utf8(sysroot.stdout).ok()?;
    let sysroot = Path::new(sysroot.trim());
    let rustlib = sysroot.join("lib").join("rustlib");
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    // Tools live under lib/rustlib/<host-triple>/bin/.
    let entries = std::fs::read_dir(&rustlib).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join("bin").join(&exe);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Compute an aggregate coverage report from every group's artifacts.
///
/// # Errors
///
/// Returns `Err(reason)` (a graceful "unavailable" explanation) when the
/// `llvm-tools` component is missing, no group produced coverage artifacts, or
/// the coverage tools fail.
pub(crate) fn compute(groups: &[GroupCoverage], scratch_root: &Path) -> Result<CoverageReport, String> {
    if groups.is_empty() {
        return Err("no compiled target produced coverage data (command targets are not instrumented)".into());
    }
    let Some(profdata_tool) = llvm_tool("llvm-profdata") else {
        return Err("llvm-tools component not installed (run: rustup component add llvm-tools-preview)".into());
    };
    let Some(cov_tool) = llvm_tool("llvm-cov") else {
        return Err("llvm-tools component not installed (run: rustup component add llvm-tools-preview)".into());
    };

    // Gather all profraw files across groups.
    let mut profraws: Vec<PathBuf> = Vec::new();
    for g in groups {
        if let Ok(rd) = std::fs::read_dir(&g.profraw_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("profraw") {
                    profraws.push(p);
                }
            }
        }
    }
    if profraws.is_empty() {
        return Err("instrumented run produced no .profraw profiles".into());
    }

    // Merge into a single indexed profile. A large spec can produce hundreds of
    // .profraw files — far more than fit on a command line (Windows caps the
    // command line at ~32K chars) — so pass them via an `-f` input-file list
    // (one path per line) rather than as individual arguments.
    let profdata = scratch_root.join("merged.profdata");
    let list_file = scratch_root.join("profraw-list.txt");
    let list = profraws
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_file, list).map_err(|e| format!("failed to write profraw list: {e}"))?;
    let mut merge = Command::new(&profdata_tool);
    merge.arg("merge").arg("-sparse").arg("-f").arg(&list_file).arg("-o").arg(&profdata);
    let out = merge.output().map_err(|e| format!("failed to run llvm-profdata: {e}"))?;
    if !out.status.success() {
        return Err(format!("llvm-profdata merge failed: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // Export coverage for all runner binaries against the merged profile, then
    // filter to the implementation sources ourselves (llvm-cov's `export`
    // source-filtering is unreliable, so we scope by matching file paths).
    let mut export = Command::new(&cov_tool);
    export.arg("export");
    let mut first = true;
    for g in groups {
        if first {
            export.arg(&g.binary);
            first = false;
        } else {
            export.arg("-object").arg(&g.binary);
        }
    }
    export.arg(format!("-instr-profile={}", profdata.display()));
    export.arg("--summary-only");
    let out = export.output().map_err(|e| format!("failed to run llvm-cov: {e}"))?;
    if !out.status.success() {
        return Err(format!("llvm-cov export failed: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // The implementation sources to keep, canonicalized for robust comparison.
    let mut wanted: Vec<PathBuf> = groups
        .iter()
        .flat_map(|g| g.sources.iter())
        .map(|s| std::fs::canonicalize(s).unwrap_or_else(|_| s.clone()))
        .collect();
    wanted.sort();
    wanted.dedup();

    parse_export(&out.stdout, &wanted)
}

/// True if `file` (a path from llvm-cov JSON) is one of the `wanted`
/// implementation sources. Compares canonicalized absolute paths. No file-name
/// fallback: common names like `lib.rs`/`mod.rs` would match across crates.
fn is_wanted(file: &str, wanted: &[PathBuf]) -> bool {
    let file_canon = std::fs::canonicalize(file).unwrap_or_else(|_| PathBuf::from(file));
    wanted.contains(&file_canon)
}

/// Parse `llvm-cov export --summary-only` JSON into a [`CoverageReport`],
/// keeping only the implementation source files and recomputing totals from
/// them (so the generated runner and unrelated crate files are excluded).
fn parse_export(json: &[u8], wanted: &[PathBuf]) -> Result<CoverageReport, String> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| format!("failed to parse llvm-cov JSON: {e}"))?;
    let data = v.get("data").and_then(|d| d.get(0)).ok_or("llvm-cov JSON missing data[0]")?;

    let mut files = Vec::new();
    let mut lines_total = 0u64;
    let mut lines_covered = 0u64;
    if let Some(arr) = data.get("files").and_then(serde_json::Value::as_array) {
        for f in arr {
            let Some(path) = f.get("filename").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if !is_wanted(path, wanted) {
                continue;
            }
            let l = f.get("summary").and_then(|s| s.get("lines"));
            let (count, covered, pct) = l.map_or((0, 0, 0.0), |l| {
                (
                    l.get("count").and_then(serde_json::Value::as_u64).unwrap_or(0),
                    l.get("covered").and_then(serde_json::Value::as_u64).unwrap_or(0),
                    l.get("percent").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                )
            });
            lines_total += count;
            lines_covered += covered;
            files.push(FileCoverage {
                path: path.to_string(),
                lines_total: count,
                lines_covered: covered,
                percent: pct,
            });
        }
    }

    if files.is_empty() {
        return Err("no implementation source files appeared in the coverage report".into());
    }

    #[allow(clippy::cast_precision_loss)]
    let percent = if lines_total == 0 {
        0.0
    } else {
        (lines_covered as f64 / lines_total as f64) * 100.0
    };

    Ok(CoverageReport {
        lines_total,
        lines_covered,
        percent,
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic `llvm-cov export --summary-only` JSON with three files.
    const SAMPLE: &str = r#"{
        "data": [{
            "files": [
                { "filename": "/c1/src/lib.rs", "summary": { "lines": { "count": 10, "covered": 4, "percent": 40.0 } } },
                { "filename": "/c2/src/lib.rs", "summary": { "lines": { "count": 20, "covered": 15, "percent": 75.0 } } },
                { "filename": "/other/src/lib.rs", "summary": { "lines": { "count": 100, "covered": 0, "percent": 0.0 } } }
            ]
        }]
    }"#;

    #[test]
    fn parse_export_unions_and_filters_multiple_roots() {
        // Two wanted crates (multi-package-root union); a third file from an
        // unrelated crate — also named lib.rs — must be excluded, proving the
        // filter matches full paths, not file names.
        let wanted = vec![PathBuf::from("/c1/src/lib.rs"), PathBuf::from("/c2/src/lib.rs")];
        let report = parse_export(SAMPLE.as_bytes(), &wanted).expect("parse");
        assert_eq!(report.files.len(), 2, "the unrelated lib.rs must be excluded");
        assert_eq!(report.lines_total, 30, "totals summed across both wanted crates");
        assert_eq!(report.lines_covered, 19);
        assert!((report.percent - 63.333).abs() < 0.01, "percent: {}", report.percent);
    }

    #[test]
    fn parse_export_errors_when_no_wanted_file_present() {
        let wanted = vec![PathBuf::from("/nowhere/src/lib.rs")];
        assert!(parse_export(SAMPLE.as_bytes(), &wanted).is_err());
    }

    #[test]
    fn is_wanted_matches_exact_path_not_file_name() {
        let wanted = vec![PathBuf::from("/c1/src/lib.rs")];
        assert!(is_wanted("/c1/src/lib.rs", &wanted));
        // Same file name, different crate — must NOT match.
        assert!(!is_wanted("/c2/src/lib.rs", &wanted));
    }

    #[test]
    fn collect_crate_sources_skips_target_and_tests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src").join("sub")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("src").join("a.rs"), "// a").unwrap();
        std::fs::write(root.join("src").join("sub").join("b.rs"), "// b").unwrap();
        std::fs::write(root.join("target").join("gen.rs"), "// build artifact").unwrap();
        std::fs::write(root.join("tests").join("t.rs"), "// test code").unwrap();

        let mut names: Vec<String> = collect_crate_sources(root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["a.rs".to_string(), "b.rs".to_string()],
            "only src/*.rs, skipping target/ and tests/"
        );
    }
}
