//! Typed failures from the capture API.
use specgate::SpecEvent;

/// Semantic stage at which capture failed.
///
/// This taxonomy is public for CTSC registry compatibility. Capture failures
/// also expose focused predicates so callers need not exhaustively match it.
///
/// # Examples
/// ```
/// use specgate_cli::capture_error::CaptureErrorKind;
/// let kind = CaptureErrorKind::Request;
/// match kind {
///     CaptureErrorKind::Request => {}
///     _ => println!("a newer capture error category"),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[non_exhaustive]
#[spec_event(name = "CaptureErrorKind")]
pub enum CaptureErrorKind {
    /// The capture request could not be interpreted.
    Request,
    /// Binding resolution or target discovery failed.
    Discovery,
    /// Component selection failed.
    Selection,
    /// Building, enumerating, or running tests failed.
    Execution,
    /// Bundle encoding or artifact writing failed.
    Encoding,
}
