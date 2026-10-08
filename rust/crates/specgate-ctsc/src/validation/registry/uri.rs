//! Resolve local-only registry `file:` retrieval hints.
//!
//! Relative paths use the importing registry's directory. Localhost and platform-native
//! absolute forms are accepted; remote authorities, UNC paths, queries, fragments, and
//! invalid UTF-8 are rejected. Non-file schemes return `Ok(None)`.

use super::{Filesystem, canonicalize};
use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};
use templated_uri::Authority;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    Policy,
    Authority,
    Utf8,
}

/// Structured local-file URI resolution failure retaining upstream parse sources.
#[ohno::error]
#[display("{message}")]
pub(super) struct Error {
    kind: ErrorKind,
    message: String,
}
impl Error {
    fn sourced(kind: ErrorKind, message: String, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(kind, message, source)
    }
}

/// Resolve a supported local file URI, preserving path and upstream I/O context.
pub(super) fn resolve_uri(base: impl AsRef<Path>, uri: impl AsRef<str>, filesystem: &Filesystem) -> Result<Option<PathBuf>, Error> {
    let base = base.as_ref();
    let uri = uri.as_ref();
    let Some(raw) = uri.strip_prefix("file:") else {
        return Ok(None);
    };
    if raw.contains('?') || raw.contains('#') {
        return Err(invalid(format!("file URI must not contain a query or fragment: '{uri}'")));
    }
    let (authority, path) = if let Some(network) = raw.strip_prefix("//") {
        let split = network.find('/').unwrap_or(network.len());
        (&network[..split], &network[split..])
    } else {
        ("", raw)
    };
    if !authority.is_empty() {
        let parsed = authority
            .parse::<Authority>()
            .map_err(|error| Error::sourced(ErrorKind::Authority, format!("invalid file URI '{uri}': {error}"), error))?;
        if !parsed.as_str().eq_ignore_ascii_case("localhost") {
            return Err(invalid(format!(
                "automatic registry import resolution does not support network file URI '{uri}'; download the registry and supply its local path with --import"
            )));
        }
    }
    let decoded = percent_decode_str(path)
        .decode_utf8()
        .map_err(|error| Error::sourced(ErrorKind::Utf8, format!("file URI path is not UTF-8: {error}"), error))?;
    if decoded.starts_with("//") || decoded.starts_with(r"\\") {
        return Err(invalid(format!(
            "automatic registry import resolution does not support UNC or network file URI '{uri}'; download the registry and supply its local path with --import"
        )));
    }
    #[cfg(windows)]
    let decoded = decoded
        .strip_prefix('/')
        .filter(|value| is_drive(value))
        .unwrap_or(decoded.as_ref());
    #[cfg(not(windows))]
    let decoded = decoded.as_ref();
    let candidate = PathBuf::from(decoded);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        base.parent().unwrap_or_else(|| Path::new(".")).join(candidate)
    };
    Ok(Some(canonicalize(&candidate, filesystem)))
}

fn invalid(message: String) -> Error {
    Error::new(ErrorKind::Policy, message)
}

#[cfg(windows)]
fn is_drive(value: &str) -> bool {
    // A slash-prefixed Windows drive path has the form `/C:/...`; byte 1 is the colon.
    value.as_bytes().get(1) == Some(&b':')
}
