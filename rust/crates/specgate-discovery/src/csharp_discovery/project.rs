//! C# project settings and path rendering.

use crate::runner::system::System;
use std::path::{Path, PathBuf};

/// Valid C# nullable-analysis modes accepted by `MSBuild`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NullableMode {
    Disable,
    Enable,
    Warnings,
    Annotations,
}
impl NullableMode {
    fn parse(value: impl AsRef<str>) -> Option<Self> {
        match value.as_ref().trim().to_ascii_lowercase().as_str() {
            "disable" => Some(Self::Disable),
            "enable" => Some(Self::Enable),
            "warnings" => Some(Self::Warnings),
            "annotations" => Some(Self::Annotations),
            _ => None,
        }
    }
    const fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Enable => "enable",
            Self::Warnings => "warnings",
            Self::Annotations => "annotations",
        }
    }
}
impl AsRef<str> for NullableMode {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Valid SDK implicit-using modes accepted by `MSBuild`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImplicitUsings {
    Disable,
    Enable,
}
impl ImplicitUsings {
    fn parse(value: impl AsRef<str>) -> Option<Self> {
        match value.as_ref().trim().to_ascii_lowercase().as_str() {
            "disable" => Some(Self::Disable),
            "enable" => Some(Self::Enable),
            _ => None,
        }
    }
    const fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Enable => "enable",
        }
    }
}
impl AsRef<str> for ImplicitUsings {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Compiler settings inherited by the generated reflection runner.
pub(super) struct RunnerSettings {
    /// Executable target framework; open because installed SDK monikers evolve.
    pub(super) framework: String,
    /// Validated nullable-analysis mode.
    pub(super) nullable: NullableMode,
    /// Validated SDK implicit-using mode.
    pub(super) implicit_usings: ImplicitUsings,
    /// Optional compiler language version; open for SDK-specific versions.
    pub(super) lang_version: Option<String>,
}

// The reflection runner needs a modern executable target even when the fixture
// is a netstandard library; net10.0 matches the repository SDK baseline.
const RUNNER_FRAMEWORK: &str = "net10.0";
const LIBRARY_PREFIX: &str = "netstandard";
// Generated source enables the compiler contexts used to preserve nullable
// metadata and ordinary SDK implicit imports unless the fixture overrides them.
// Windows verbatim paths use this prefix to bypass ordinary path parsing; generated C# paths omit it.
const VERBATIM_PREFIX: &str = r"\\?\";

pub(super) fn runner_settings(target: &crate::binding::Target, system: &System) -> RunnerSettings {
    let project = read_settings(&target.package_root, system);
    let selected = target
        .framework
        .clone()
        .or(project.framework)
        .unwrap_or_else(|| RUNNER_FRAMEWORK.to_string());
    let framework = if selected.starts_with(LIBRARY_PREFIX) {
        RUNNER_FRAMEWORK.to_string()
    } else {
        selected
    };
    RunnerSettings {
        framework,
        nullable: project
            .nullable
            .as_deref()
            .and_then(NullableMode::parse)
            .unwrap_or(NullableMode::Enable),
        implicit_usings: project
            .implicit_usings
            .as_deref()
            .and_then(ImplicitUsings::parse)
            .unwrap_or(ImplicitUsings::Enable),
        lang_version: project.lang_version,
    }
}

#[derive(Default)]
struct ProjectSettings {
    framework: Option<String>,
    nullable: Option<String>,
    implicit_usings: Option<String>,
    lang_version: Option<String>,
}

fn read_settings(package_root: impl AsRef<Path>, system: &System) -> ProjectSettings {
    let package_root = package_root.as_ref();
    let Ok(entries) = system.filesystem.read_dir(package_root) else {
        return ProjectSettings::default();
    };
    for path in entries.into_iter().filter_map(Result::ok) {
        if path.extension().and_then(|extension| extension.to_str()) != Some("csproj") {
            continue;
        }
        let Ok(text) = system.filesystem.read_to_string(&path) else {
            return ProjectSettings::default();
        };
        return ProjectSettings {
            framework: extract_tag(&text, "TargetFramework").or_else(|| {
                extract_tag(&text, "TargetFrameworks")
                    .and_then(|frameworks| frameworks.split(';').next().map(str::trim).map(str::to_string))
            }),
            nullable: extract_tag(&text, "Nullable"),
            implicit_usings: extract_tag(&text, "ImplicitUsings"),
            lang_version: extract_tag(&text, "LangVersion"),
        };
    }
    ProjectSettings::default()
}

pub(super) fn extract_tag(text: impl AsRef<str>, tag: impl AsRef<str>) -> Option<String> {
    let text = text.as_ref();
    let tag = tag.as_ref();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)?;
    Some(text[start..start + end].trim().to_string())
}

pub(super) fn slash_path(path: impl AsRef<Path>, system: &System) -> String {
    let path = path.as_ref();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        system
            .current_directory
            .get()
            .map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    let display = absolute.display().to_string();
    display.strip_prefix(VERBATIM_PREFIX).unwrap_or(&display).replace('\\', "/")
}

pub(super) fn strip_prefix(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let value = path.to_string_lossy();
    value
        .strip_prefix(VERBATIM_PREFIX)
        .map_or_else(|| path.to_path_buf(), PathBuf::from)
}

pub(super) fn xml_text(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    let mut escaped = String::with_capacity(value.len());
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

pub(super) fn string_literal(value: impl AsRef<str>) -> String {
    let escaped = value
        .as_ref()
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
    use crate::runner::system::{FailureKey, FakeFs, FakeProcess};
    use std::collections::{BTreeMap, VecDeque};

    fn system(filesystem: FakeFs) -> System {
        System::builder((filesystem, FakeProcess::default()))
            .current_directory(Ok(PathBuf::from("injected-cwd")))
            .process_id(1)
            .build()
    }

    #[test]
    fn root_defaults() {
        let filesystem = FakeFs::builder()
            .failures(BTreeMap::from([(
                FailureKey::test("read_dir:fixture"),
                VecDeque::from(["denied".to_string()]),
            )]))
            .build();
        let settings = runner_settings(
            &crate::binding::Target {
                package_root: PathBuf::from("fixture"),
                runtime: crate::binding::Runtime::Smol,
                framework: None,
                command: None,
                outputs: None,
            },
            &system(filesystem),
        );
        assert_eq!(settings.framework, "net10.0");
        assert_eq!(settings.nullable, NullableMode::Enable);
        assert_eq!(settings.implicit_usings, ImplicitUsings::Enable);
        assert_eq!(settings.lang_version, None);
    }

    #[test]
    fn settings_retained() {
        let project = PathBuf::from("fixture/Usable.csproj");
        let filesystem = FakeFs::builder()
            .files(BTreeMap::from([(
                project.clone(),
                br"<Project><PropertyGroup><TargetFramework>net9.0</TargetFramework><Nullable>annotations</Nullable><ImplicitUsings>disable</ImplicitUsings><LangVersion>preview</LangVersion></PropertyGroup></Project>".to_vec(),
            )]))
            .directory_results(BTreeMap::from([(
                PathBuf::from("fixture"),
                vec![Err("entry denied".to_string()), Ok(project)],
            )]))
            .build();
        let settings = runner_settings(
            &crate::binding::Target {
                package_root: PathBuf::from("fixture"),
                runtime: crate::binding::Runtime::Smol,
                framework: None,
                command: None,
                outputs: None,
            },
            &system(filesystem),
        );
        assert_eq!(settings.framework, "net9.0");
        assert_eq!(settings.nullable, NullableMode::Annotations);
        assert_eq!(settings.implicit_usings, ImplicitUsings::Disable);
        assert_eq!(settings.lang_version.as_deref(), Some("preview"));
    }

    #[test]
    fn rendering_preserved() {
        assert_eq!(strip_prefix(Path::new(r"\\?\C:\work\fixture")), PathBuf::from(r"C:\work\fixture"));
        assert_eq!(string_literal("a\\b\"c\n"), "\"a\\\\b\\\"c\\n\"");
        assert_eq!(xml_text("<&>\"'"), "&lt;&amp;&gt;&quot;&apos;");
    }

    #[test]
    fn relative_path() {
        assert_eq!(
            slash_path(Path::new("relative/fixture.dll"), &system(FakeFs::default())),
            "injected-cwd/relative/fixture.dll"
        );
    }

    #[test]
    fn missing_directory() {
        let system = System::builder((FakeFs::default(), FakeProcess::default()))
            .current_directory(Err("cwd denied".to_string()))
            .process_id(1)
            .build();
        assert_eq!(slash_path(Path::new("relative/fixture.dll"), &system), "relative/fixture.dll");
    }

    #[test]
    fn absolute_path() {
        #[cfg(windows)]
        let absolute = Path::new(r"\\?\C:\work\fixture.dll");
        #[cfg(not(windows))]
        let absolute = Path::new("/work/fixture.dll");
        #[cfg(windows)]
        let expected = "C:/work/fixture.dll";
        #[cfg(not(windows))]
        let expected = "/work/fixture.dll";
        assert_eq!(slash_path(absolute, &system(FakeFs::default())), expected);
    }

    #[test]
    fn unreadable_defaults() {
        let project = PathBuf::from("fixture/Unreadable.csproj");
        let filesystem = FakeFs::builder()
            .directories(BTreeMap::from([(PathBuf::from("fixture"), vec![project.clone()])]))
            .failures(BTreeMap::from([(
                FailureKey::test("read_to_string:Unreadable.csproj"),
                VecDeque::from(["denied".to_string()]),
            )]))
            .build();
        let settings = runner_settings(
            &crate::binding::Target {
                package_root: PathBuf::from("fixture"),
                runtime: crate::binding::Runtime::Smol,
                framework: None,
                command: None,
                outputs: None,
            },
            &system(filesystem),
        );
        assert_eq!(settings.framework, "net10.0");
        assert_eq!(settings.nullable, NullableMode::Enable);
        assert_eq!(settings.implicit_usings, ImplicitUsings::Enable);
        assert_eq!(settings.lang_version, None);
    }
}
