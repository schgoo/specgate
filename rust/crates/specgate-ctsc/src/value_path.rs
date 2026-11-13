//! Unambiguous literal segments shared by validation and comparison diagnostics.

/// Append a source name without confusing it with synthetic wrappers or indexes.
pub(crate) fn literal_path(parent: &str, name: &str) -> String {
    let ordinary = !name.is_empty()
        && name.bytes().enumerate().all(|(index, ch)| {
            ch == b'_'
                || if index == 0 {
                    ch.is_ascii_alphabetic()
                } else {
                    ch.is_ascii_alphanumeric()
                }
        })
        && !matches!(
            name,
            "some" | "none" | "ok" | "err" | "Some" | "None" | "Ok" | "Err" | "result" | "unit"
        );
    if ordinary {
        format!("{parent}.{name}")
    } else {
        format!("{parent}[{}]", serde_json::to_string(name).expect("a string serializes"))
    }
}

#[cfg(test)]
mod tests {
    use super::literal_path;

    #[test]
    fn literal_segments() {
        for name in ["ordinary", "_field2", "AZaz09"] {
            assert_eq!(literal_path("$", name), format!("$.{name}"));
        }
        for name in [
            "some",
            "none",
            "ok",
            "err",
            "Some",
            "None",
            "Ok",
            "Err",
            "result",
            "unit",
            "a.b",
            "a[0]",
            "a b",
            "",
            "0",
            "1a",
            "q\"\\",
            "\n\t\0",
            "😀",
            "한글",
            "café",
            "\u{0345}",
            "\u{05b0}",
            "a\u{0345}",
            "a\u{05b0}",
        ] {
            let path = literal_path("$", name);
            assert!(path.starts_with("$[\""), "{path}");
            assert!(path.ends_with("\"]"), "{path}");
            assert_eq!(serde_json::from_str::<String>(&path[2..path.len() - 1]).expect("JSON string"), name);
        }
    }
}
