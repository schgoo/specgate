//! Deterministic candidate trace encoding and publication.
use super::{Capture, CommandEnvironment, Path, Plan, Report, encode_otlp, write_atomic};
use super::{Error, ErrorKind};

// CTSC target identities reserve this discriminator for replay outputs; changing it alters golden trace bytes.
const CANDIDATE_TRACE: &str = "candidate";

/// Candidate target label: the output file name without its `.json` and `.otlp` suffixes.
///
/// The label always agrees with the published path, matching how capture
/// labels a bundle with its directory name.
pub(super) fn candidate_label(out: &Path) -> Result<&str, Error> {
    let name = out
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| Error::new_message(ErrorKind::Request, "replay output must end in a UTF-8 file name"))?;
    let name = name.strip_suffix(".json").unwrap_or(name);
    let label = name.strip_suffix(".otlp").unwrap_or(name);
    if label.is_empty() {
        return Err(Error::new_message(
            ErrorKind::Request,
            format!("replay output file name {name} has no label before its .otlp.json suffix"),
        ));
    }
    Ok(label)
}

/// Encode replayed native captures as a CTSC candidate trace, publish it atomically, and report its contents.
///
/// The captures are encoded with the plan's package and registry identities and a target label taken from
/// the output file name. The returned report describes
/// the published scenarios, operations, plans, and output path.
///
/// # Errors
/// Returns an error when CTSC encoding fails, counts exceed their wire widths, or the output cannot be
/// converted and atomically written through the supplied command environment.
pub(super) fn write(
    plan: &Plan,
    captures: impl AsRef<[Capture]>,
    out: impl AsRef<Path>,
    system: &CommandEnvironment,
) -> Result<Report, Error> {
    let out = out.as_ref();
    let target_identity = format!("{CANDIDATE_TRACE}:{}:{}", plan.target.package_name, candidate_label(out)?);
    let metadata = specgate_ctsc::capture::Metadata::new(
        env!("CARGO_PKG_VERSION"),
        specgate_ctsc::capture::Target::new(&target_identity, plan.target.language.as_str()),
        specgate_ctsc::capture::Registry::new(
            plan.registry_id.as_str(),
            plan.registry_version.as_str(),
            plan.registry_digest.as_str(),
        ),
    );
    let encoded = encode_otlp(captures.as_ref(), &metadata)?;
    write_atomic(out, encoded.otlp_json.as_bytes(), system)?;

    let scenarios = u32::try_from(plan.scenarios.len()).map_err(|_error| "replayed scenario count exceeds u32".to_string())?;
    let operation_count = plan.scenarios.iter().try_fold(0_usize, |count, scenario| {
        count
            .checked_add(scenario.operations.len())
            .ok_or_else(|| "replayed operation count overflow".to_string())
    })?;
    let operations = u32::try_from(operation_count).map_err(|_error| "replayed operation count exceeds u32".to_string())?;
    let plans = u32::try_from(plan.links.len()).map_err(|_error| "replay plan count exceeds u32".to_string())?;
    Ok(Report {
        component_id: specgate::ComponentId::from(plan.component_id.as_str()),
        scenarios,
        operations,
        plans,
        output_path: out.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Path, candidate_label};

    #[test]
    fn labels_strip_suffixes_and_reject_empty() {
        assert_eq!(candidate_label(Path::new("dir/run-1.otlp.json")).unwrap(), "run-1");
        assert_eq!(candidate_label(Path::new("run-2.json")).unwrap(), "run-2");
        assert_eq!(candidate_label(Path::new("plain")).unwrap(), "plain");
        for empty in [".json", ".otlp.json", "dir/.json"] {
            let error = candidate_label(Path::new(empty)).unwrap_err();
            assert!(error.to_string().contains("has no label"), "{empty}: {error}");
        }
    }
}
