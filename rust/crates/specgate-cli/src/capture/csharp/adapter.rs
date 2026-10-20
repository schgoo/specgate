//! Test-framework selection for the C# capture runner.
//!
//! Capture runs each listed test alone through `dotnet test --filter`. The
//! filter grammar and the escaping are `VSTest`'s and identical for every
//! adapter; what differs is which test property holds the listed name:
//! xUnit adapters match it with `DisplayName`, while the `MSTest` and `NUnit`
//! adapters do not implement `DisplayName` and match it with `Name`.
//!
//! The adapter is detected from the tests project's evaluated
//! `PackageReference` items, so the binding needs no field for it. A project
//! that references no known framework, or several, is rejected.
use super::{FailureContext, Path};

/// The test frameworks whose selection capture can prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Adapter {
    XUnit,
    MSTest,
    NUnit,
}

impl Adapter {
    /// The `--filter` property that equals the name `--list-tests` prints.
    pub(super) const fn filter_property(self) -> &'static str {
        match self {
            Self::XUnit => "DisplayName",
            Self::MSTest | Self::NUnit => "Name",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::XUnit => "xUnit",
            Self::MSTest => "MSTest",
            Self::NUnit => "NUnit",
        }
    }

    /// The framework a package belongs to, by case-insensitive package id.
    fn of_package(package: &str) -> Option<Self> {
        let package = package.to_ascii_lowercase();
        match package.as_str() {
            "xunit" | "xunit.core" | "xunit.runner.visualstudio" => Some(Self::XUnit),
            _ if package.starts_with("xunit.v3") => Some(Self::XUnit),
            "mstest" | "mstest.testframework" | "mstest.testadapter" => Some(Self::MSTest),
            "nunit" | "nunit3testadapter" => Some(Self::NUnit),
            _ => None,
        }
    }

    /// Pick the one framework a tests project uses from its package ids.
    pub(super) fn detect<'a>(packages: impl IntoIterator<Item = &'a str>, project: &Path) -> Result<Self, FailureContext> {
        let mut found: Vec<Self> = packages.into_iter().filter_map(Self::of_package).collect();
        found.sort_by_key(|adapter| adapter.label());
        found.dedup();
        match found.as_slice() {
            [adapter] => Ok(*adapter),
            [] => Err(FailureContext::domain(format!(
                "tests project {} references no supported test framework; C# capture supports xUnit (xunit, xunit.v3), \
                 MSTest (MSTest.TestFramework), and NUnit (NUnit)",
                project.display()
            ))),
            several => Err(FailureContext::domain(format!(
                "tests project {} references several test frameworks ({}); C# capture needs exactly one",
                project.display(),
                several.iter().map(|adapter| adapter.label()).collect::<Vec<_>>().join(", ")
            ))),
        }
    }
}
