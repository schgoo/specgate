//! Source-annotation scanning (string-based, NOT interpretation).
//!
//! We only extract attribute names, function signatures and which structs
//! derive SpecEvent. We never evaluate bodies — that is the cargo
//! toolchain's job.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct AnnotatedSource {
    pub setups: BTreeMap<String, FnSig>,
    pub operations: BTreeMap<String, OpDecl>,
    /// Structs that have `#[derive(... SpecEvent ...)]`.
    pub spec_event_structs: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct FnSig {
    pub fn_ident: String,
    pub params: Vec<(String, String)>, // (param_name, type_text)
    pub return_type: String,           // "" if unit
}

#[derive(Debug, Clone)]
pub struct OpDecl {
    pub sig: FnSig,
    /// `Some(struct)` if the op is a method on that struct, `None` if free fn.
    pub method_of: Option<String>,
    pub takes_self: bool,
}

/// Tokenise into significant Rust tokens, ignoring comments and string
/// literals beyond the bits we need.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

pub fn scan(src: &str) -> AnnotatedSource {
    let src = strip_comments(src);
    let mut setups = BTreeMap::new();
    let mut operations = BTreeMap::new();
    let mut spec_event_structs = std::collections::BTreeSet::new();

    // SpecEvent-derived structs:
    //   #[derive(... SpecEvent ...)] ... struct <NAME>
    for cap in find_iter(&src, "#[derive(") {
        let after = &src[cap..];
        let close = match after.find(")]") {
            Some(p) => p,
            None => continue,
        };
        let derive_list = &after[9..close];
        if !derive_list.split(',').any(|t| t.trim() == "SpecEvent") {
            continue;
        }
        let rest = &after[close + 2..];
        if let Some(struct_name) = scan_struct_name(rest) {
            spec_event_structs.insert(struct_name);
        }
    }

    // Top-level attribute-driven items:
    //   #[spec_setup("name")] (pub )? fn NAME(...)
    //   #[spec_operation("name")] ... fn NAME(...) -> ...
    //
    // Methods (inside `impl <Struct> {` blocks) are recognised by tracking
    // the surrounding impl block.
    let mut current_impl: Vec<String> = Vec::new();
    let mut depth_to_impl_target: Vec<Option<String>> = vec![None];

    let mut i = 0usize;
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();

    while i < n {
        let c = chars[i];
        if c == '{' {
            depth_to_impl_target.push(current_impl.last().cloned());
            current_impl.clear();
            i += 1;
            continue;
        }
        if c == '}' {
            depth_to_impl_target.pop();
            i += 1;
            continue;
        }

        // Detect `impl <TYPE> {`
        if starts_word_at(&chars, i, "impl") {
            let mut j = i + 4;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            // Skip generics like <T>
            if j < n && chars[j] == '<' {
                let mut depth = 1;
                j += 1;
                while j < n && depth > 0 {
                    if chars[j] == '<' { depth += 1; }
                    if chars[j] == '>' { depth -= 1; }
                    j += 1;
                }
                while j < n && chars[j].is_whitespace() { j += 1; }
            }
            // Read type name (single ident, optionally followed by generics).
            let start = j;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let ty: String = chars[start..j].iter().collect();
            // Skip optional generics on the type
            while j < n && chars[j].is_whitespace() { j += 1; }
            if j < n && chars[j] == '<' {
                let mut depth = 1;
                j += 1;
                while j < n && depth > 0 {
                    if chars[j] == '<' { depth += 1; }
                    if chars[j] == '>' { depth -= 1; }
                    j += 1;
                }
            }
            // The next `{` belongs to this impl.
            while j < n && chars[j] != '{' {
                j += 1;
            }
            if j < n && !ty.is_empty() {
                current_impl.clear();
                current_impl.push(ty);
            }
            i = j;
            continue;
        }

        // Detect attribute: #[spec_setup("NAME")] or #[spec_operation("NAME")]
        if c == '#' && i + 1 < n && chars[i + 1] == '[' {
            // Find the closing ].
            let mut j = i + 2;
            let mut depth = 1;
            while j < n && depth > 0 {
                if chars[j] == '[' { depth += 1; }
                if chars[j] == ']' { depth -= 1; }
                j += 1;
            }
            let attr: String = chars[i + 2..j - 1].iter().collect();
            let attr_trim = attr.trim();
            let (kind, name) = if let Some(n) = attr_inner(attr_trim, "spec_setup") {
                (Some("setup"), Some(n))
            } else if let Some(n) = attr_inner(attr_trim, "spec_operation") {
                (Some("op"), Some(n))
            } else {
                (None, None)
            };
            if let (Some(k), Some(name)) = (kind, name) {
                // Find the following `fn ident(...) -> ret`.
                let mut p = j;
                // Skip whitespace and other outer attributes / visibility.
                loop {
                    while p < n && chars[p].is_whitespace() {
                        p += 1;
                    }
                    if p < n && chars[p] == '#' {
                        // skip another attribute
                        let mut d = 1;
                        let mut q = p + 2;
                        while q < n && d > 0 {
                            if chars[q] == '[' { d += 1; }
                            if chars[q] == ']' { d -= 1; }
                            q += 1;
                        }
                        p = q;
                        continue;
                    }
                    if starts_word_at(&chars, p, "pub") {
                        p += 3;
                        while p < n && chars[p].is_whitespace() { p += 1; }
                        if p < n && chars[p] == '(' {
                            let mut d = 1;
                            p += 1;
                            while p < n && d > 0 {
                                if chars[p] == '(' { d += 1; }
                                if chars[p] == ')' { d -= 1; }
                                p += 1;
                            }
                        }
                        continue;
                    }
                    // Skip leading function qualifiers (`async`, `const`, `unsafe`,
                    // `extern "C"`) so `#[spec_operation] async fn foo` is
                    // recognised.
                    if starts_word_at(&chars, p, "async")
                        || starts_word_at(&chars, p, "const")
                        || starts_word_at(&chars, p, "unsafe")
                    {
                        let word_len = if starts_word_at(&chars, p, "async") {
                            5
                        } else if starts_word_at(&chars, p, "const") {
                            5
                        } else {
                            6
                        };
                        p += word_len;
                        continue;
                    }
                    if starts_word_at(&chars, p, "extern") {
                        p += 6;
                        while p < n && chars[p].is_whitespace() { p += 1; }
                        // Optional ABI string.
                        if p < n && chars[p] == '"' {
                            p += 1;
                            while p < n && chars[p] != '"' { p += 1; }
                            if p < n { p += 1; }
                        }
                        continue;
                    }
                    break;
                }
                if !starts_word_at(&chars, p, "fn") {
                    i = j;
                    continue;
                }
                p += 2;
                while p < n && chars[p].is_whitespace() { p += 1; }
                let ident_start = p;
                while p < n && (chars[p].is_alphanumeric() || chars[p] == '_') {
                    p += 1;
                }
                let fn_ident: String = chars[ident_start..p].iter().collect();
                while p < n && chars[p] != '(' { p += 1; }
                if p >= n {
                    i = j;
                    continue;
                }
                // Collect parens (balanced).
                let paren_start = p + 1;
                let mut d = 1;
                p += 1;
                while p < n && d > 0 {
                    if chars[p] == '(' { d += 1; }
                    if chars[p] == ')' { d -= 1; }
                    p += 1;
                }
                let paren_end = p - 1;
                let param_text: String = chars[paren_start..paren_end].iter().collect();
                // Return type.
                let mut ret_text = String::new();
                while p < n && chars[p].is_whitespace() { p += 1; }
                if p + 1 < n && chars[p] == '-' && chars[p + 1] == '>' {
                    p += 2;
                    while p < n && chars[p].is_whitespace() { p += 1; }
                    let r_start = p;
                    let mut depth_ang = 0i32;
                    while p < n {
                        let cc = chars[p];
                        if cc == '<' { depth_ang += 1; }
                        if cc == '>' { depth_ang -= 1; }
                        if depth_ang <= 0 && (cc == '{' || cc == ';' || cc == 'w') {
                            // `w` for "where"
                            if cc == 'w' && !starts_word_at(&chars, p, "where") {
                                p += 1;
                                continue;
                            }
                            break;
                        }
                        p += 1;
                    }
                    ret_text = chars[r_start..p].iter().collect::<String>().trim().to_string();
                }

                let (params, takes_self) = parse_params(&param_text);
                let method_of = if takes_self {
                    depth_to_impl_target
                        .last()
                        .cloned()
                        .flatten()
                        .filter(|s| !s.is_empty())
                } else {
                    None
                };
                let sig = FnSig {
                    fn_ident: fn_ident.clone(),
                    params,
                    return_type: ret_text,
                };
                match k {
                    "setup" => {
                        setups.insert(name, sig);
                    }
                    "op" => {
                        operations.insert(
                            name,
                            OpDecl {
                                sig,
                                method_of,
                                takes_self,
                            },
                        );
                    }
                    _ => {}
                }
                i = j;
                continue;
            }
            i = j;
            continue;
        }

        i += 1;
    }

    AnnotatedSource {
        setups,
        operations,
        spec_event_structs,
    }
}

/// Returns the inner string of `#[NAME("INNER")]` if the attr matches NAME.
fn attr_inner(attr: &str, name: &str) -> Option<String> {
    let stripped = attr.strip_prefix(name)?;
    let stripped = stripped.trim_start();
    let stripped = stripped.strip_prefix('(')?;
    let stripped = stripped.trim_start();
    let stripped = stripped.strip_prefix('"')?;
    let end = stripped.find('"')?;
    Some(stripped[..end].to_string())
}

fn parse_params(text: &str) -> (Vec<(String, String)>, bool) {
    let mut out = Vec::new();
    let mut takes_self = false;
    let mut depth_ang = 0i32;
    let mut depth_par = 0i32;
    let mut depth_brk = 0i32;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '<' => depth_ang += 1,
            '>' => depth_ang -= 1,
            '(' => depth_par += 1,
            ')' => depth_par -= 1,
            '[' => depth_brk += 1,
            ']' => depth_brk -= 1,
            _ => {}
        }
        if c == ',' && depth_ang == 0 && depth_par == 0 && depth_brk == 0 {
            push_param(&cur, &mut out, &mut takes_self);
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        push_param(&cur, &mut out, &mut takes_self);
    }
    (out, takes_self)
}

fn push_param(p: &str, out: &mut Vec<(String, String)>, takes_self: &mut bool) {
    let s = p.trim();
    if s.is_empty() {
        return;
    }
    if s == "self" || s == "&self" || s == "&mut self" || s == "mut self" {
        *takes_self = true;
        return;
    }
    let (name, ty) = match s.find(':') {
        Some(i) => (s[..i].trim().to_string(), s[i + 1..].trim().to_string()),
        None => (s.to_string(), String::new()),
    };
    // Strip leading `mut ` etc. on name.
    let name = name.trim_start_matches("mut ").trim().to_string();
    out.push((name, ty));
}

fn scan_struct_name(rest: &str) -> Option<String> {
    // Skip whitespace, attributes, pub.
    let s = rest.trim_start();
    let s = match s.strip_prefix("pub") {
        Some(r) => {
            let r = r.trim_start();
            if r.starts_with('(') {
                let end = r.find(')')?;
                r[end + 1..].trim_start()
            } else {
                r
            }
        }
        None => s,
    };
    let s = s.strip_prefix("struct")?;
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    Some(s[..end].to_string())
}

fn find_iter(src: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = src[from..].find(needle) {
        let abs = from + p;
        out.push(abs);
        from = abs + needle.len();
    }
    out
}

fn starts_word_at(chars: &[char], i: usize, w: &str) -> bool {
    let bytes = w.as_bytes();
    if i + bytes.len() > chars.len() {
        return false;
    }
    for (k, &b) in bytes.iter().enumerate() {
        if chars[i + k] != b as char {
            return false;
        }
    }
    // boundary
    if i + bytes.len() < chars.len() {
        let c = chars[i + bytes.len()];
        if c.is_alphanumeric() || c == '_' {
            return false;
        }
    }
    if i > 0 {
        let c = chars[i - 1];
        if c.is_alphanumeric() || c == '_' {
            return false;
        }
    }
    true
}
