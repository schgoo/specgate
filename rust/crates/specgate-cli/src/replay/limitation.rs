//! Stable classification of intentionally unsupported replay behavior.
//!
//! Scheduling limitations are identified through typed upstream causes.
//! Unknown failures must not be mistaken for accepted limitations.

/// Stable categories for replay limitations that a caller may intentionally
/// declare. Errors outside these known limitations remain unclassified and
/// must not be accepted as evidence that replay is unsupported.
#[cfg(any(test, feature = "test-util"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayLimitation {
    StructuredValue,
    SetupOperation,
    MethodOperation,
    AsyncOperation,
    UnsupportedLanguage,
}

#[cfg(any(test, feature = "test-util"))]
impl ReplayLimitation {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::StructuredValue => "structured-value",
            Self::SetupOperation => "setup-backed-operation",
            Self::MethodOperation => "method-operation",
            Self::AsyncOperation => "async-operation",
            Self::UnsupportedLanguage => "unsupported-language",
        }
    }
}

// These fragments are emitted by the replay linker (`link.rs`) and candidate
// checks (`execution.rs`). They form part of golden limitation classification;
// producer wording and these constants must change together or unsupported
// cases become hard failures rather than accepted limitations.
#[cfg(any(test, feature = "test-util"))]
const STRUCTURED_TYPE: &str = "uses unsupported structured type";
#[cfg(any(test, feature = "test-util"))]
const REPLAY_TYPE: &str = "uses unsupported structured replay type";
#[cfg(any(test, feature = "test-util"))]
const SETUP_OPERATION: &str = "is setup-backed; replay does not yet construct setups";
#[cfg(any(test, feature = "test-util"))]
const METHOD_OPERATION: &str = "is a method; replay supports only free functions";
const RUST_CANDIDATE: &str = "replay currently supports only Rust candidates; implementation language is '";

/// Classify only stable, intentional replay limitations.
///
/// This deliberately does not have a catch-all category: malformed bundles,
/// discovery failures, missing operations, type mismatches, and runner errors
/// are unrelated defects and must fail callers such as the golden gate.
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn classify(reason: &super::Error) -> Option<ReplayLimitation> {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(reason);
    while let Some(error) = cause {
        if error.is::<specgate_ctsc::replay::UnsupportedScheduling>() {
            return Some(ReplayLimitation::AsyncOperation);
        }
        cause = error.source();
    }
    let reason = reason.to_string();
    if reason.contains(STRUCTURED_TYPE) || reason.contains(REPLAY_TYPE) {
        Some(ReplayLimitation::StructuredValue)
    } else if reason.contains(SETUP_OPERATION) {
        Some(ReplayLimitation::SetupOperation)
    } else if reason.contains(METHOD_OPERATION) {
        Some(ReplayLimitation::MethodOperation)
    } else if reason.starts_with(RUST_CANDIDATE) {
        Some(ReplayLimitation::UnsupportedLanguage)
    } else {
        None
    }
}
