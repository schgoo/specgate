//! C# project text and path helpers shared by the discovery driver.

use std::path::{Path, PathBuf};

/// Windows verbatim paths use this prefix to bypass ordinary path parsing.
const VERBATIM_PREFIX: &str = r"\\?\";

/// Read the first `<tag>…</tag>` value out of project text.
pub(super) fn extract_tag(text: impl AsRef<str>, tag: impl AsRef<str>) -> Option<String> {
    let text = text.as_ref();
    let tag = tag.as_ref();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)?;
    Some(text[start..start + end].trim().to_string())
}

/// Drop the Windows verbatim prefix so `dotnet` receives an ordinary path.
pub(super) fn strip_prefix(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let value = path.to_string_lossy();
    value
        .strip_prefix(VERBATIM_PREFIX)
        .map_or_else(|| path.to_path_buf(), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_extraction() {
        let text = "<Project><PropertyGroup><AssemblyName> Renamed </AssemblyName></PropertyGroup></Project>";
        assert_eq!(extract_tag(text, "AssemblyName").as_deref(), Some("Renamed"));
        assert_eq!(extract_tag(text, "TargetFramework"), None);
        assert_eq!(extract_tag("<AssemblyName>unterminated", "AssemblyName"), None);
    }

    #[test]
    fn verbatim_prefix_removed() {
        assert_eq!(strip_prefix(Path::new(r"\\?\C:\work\fixture")), PathBuf::from(r"C:\work\fixture"));
        assert_eq!(strip_prefix(Path::new("plain/path")), PathBuf::from("plain/path"));
    }
}
