//! Native implementation discovery for `SpecGate`'s CTSC workflow.
//!
//! This crate owns the strict target-binding resolver, Rust link-time metadata
//! discovery, C# compiled-assembly reflection discovery, raw invocation
//! metadata, and deterministic semantic schema normalization. It deliberately
//! has no dependency on `.spec.yaml`, cases, runners, matching, coverage, or
//! reports. Generated runners use invocation-unique operating-system cache
//! directories and select verified local workspace dependencies only when
//! available, otherwise using the compatible published crate version.

mod csharp_discovery;

pub mod binding;
pub mod discovery;
pub mod support;

use std::path::Path;

struct CSharpRunnerSettings {
    framework: String,
    nullable: String,
    implicit_usings: String,
    lang_version: Option<String>,
}

fn resolve_csharp_runner_settings(target: &binding::Target) -> CSharpRunnerSettings {
    const DEFAULT: &str = "net10.0";
    let project = read_csproj_settings(&target.package_root);
    let selected = target
        .framework
        .clone()
        .or(project.framework)
        .unwrap_or_else(|| DEFAULT.to_string());
    let framework = if selected.starts_with("netstandard") {
        DEFAULT.to_string()
    } else {
        selected
    };
    CSharpRunnerSettings {
        framework,
        nullable: project.nullable.unwrap_or_else(|| "enable".to_string()),
        implicit_usings: project.implicit_usings.unwrap_or_else(|| "enable".to_string()),
        lang_version: project.lang_version,
    }
}

#[derive(Default)]
struct CsProjectSettings {
    framework: Option<String>,
    nullable: Option<String>,
    implicit_usings: Option<String>,
    lang_version: Option<String>,
}

fn read_csproj_settings(package_root: &Path) -> CsProjectSettings {
    let Ok(entries) = std::fs::read_dir(package_root) else {
        return CsProjectSettings::default();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("csproj") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return CsProjectSettings::default();
        };
        return CsProjectSettings {
            framework: extract_csproj_xml_tag(&text, "TargetFramework").or_else(|| {
                extract_csproj_xml_tag(&text, "TargetFrameworks")
                    .and_then(|frameworks| frameworks.split(';').next().map(str::trim).map(str::to_string))
            }),
            nullable: extract_csproj_xml_tag(&text, "Nullable"),
            implicit_usings: extract_csproj_xml_tag(&text, "ImplicitUsings"),
            lang_version: extract_csproj_xml_tag(&text, "LangVersion"),
        };
    }
    CsProjectSettings::default()
}

fn extract_csproj_xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)?;
    Some(text[start..start + end].trim().to_string())
}

fn path_to_forward_slash(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    let display = absolute.display().to_string();
    display.strip_prefix(r"\\?\").unwrap_or(&display).replace('\\', "/")
}

fn escape_xml_text(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn csharp_string_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_project_framework_settings() {
        let cache = support::InvocationCache::create("tests", "project-settings", 0).unwrap();
        let root = cache.path();
        std::fs::write(
            root.join("Fixture.csproj"),
            "<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework><Nullable>disable</Nullable></PropertyGroup></Project>",
        )
        .unwrap();
        let settings = read_csproj_settings(root);
        assert_eq!(settings.framework.as_deref(), Some("net8.0"));
        assert_eq!(settings.nullable.as_deref(), Some("disable"));
    }
}
