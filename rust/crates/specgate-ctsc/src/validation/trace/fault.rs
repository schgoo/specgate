//! Validate extension fault names independently from trace ancestry.

// CTSC reserves this first fault-type segment for core protocol names.
const RESERVED_NAMESPACE: &str = "conformance";
// Fault types are dot-separated namespaces; later segments permit protocol-defined punctuation.
const NAMESPACE_SEPARATOR: char = '.';
const SEGMENT_PUNCTUATION: [u8; 2] = [b'_', b'-'];

/// Return whether a fault type follows the extension fault-name grammar.
///
/// Names contain at least two `.`-separated segments. The first starts with a
/// lowercase ASCII letter, subsequent characters are lowercase letters or
/// digits, and it cannot be the reserved `conformance` namespace. Thus
/// `transport.timeout` is accepted, while `conformance.timeout`, `Timeout`,
/// and `transport` are rejected.
pub(super) fn is_fault(value: impl AsRef<str>) -> bool {
    let mut segments = value.as_ref().split(NAMESPACE_SEPARATOR);
    let first = segments.next().unwrap_or_default();
    let mut remaining = segments.peekable();
    let valid_first = first != RESERVED_NAMESPACE
        && first.bytes().enumerate().all(|(index, byte)| {
            (index == 0 && byte.is_ascii_lowercase()) || (index > 0 && (byte.is_ascii_lowercase() || byte.is_ascii_digit()))
        });
    let valid_remaining = remaining.peek().is_some()
        && remaining.all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || SEGMENT_PUNCTUATION.contains(&byte))
        });
    !first.is_empty() && valid_first && valid_remaining
}
