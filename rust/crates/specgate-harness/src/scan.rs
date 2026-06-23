//! Source-annotation scanning (string-based, NOT interpretation).
//!
//! We only extract attribute names, function signatures and which structs
//! derive `SpecEvent`. We never evaluate bodies — that is the cargo
//! toolchain's job.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct AnnotatedSource {
    /// Setups discovered in the source. Each is linked to the operation it
    /// prepares; multiple setups may target one operation, and one function
    /// may carry several `#[spec_setup]` attributes (different `fills`).
    pub setups: Vec<SetupDecl>,
    pub operations: BTreeMap<String, OpDecl>,
    /// Structs that have `#[derive(... SpecEvent ...)]`.
    pub spec_event_structs: std::collections::BTreeSet<String>,
    /// Enums that have `#[derive(... SpecEvent ...)]`.
    pub spec_event_enums: std::collections::BTreeSet<String>,
}

impl AnnotatedSource {
    /// All setups linked to the given operation.
    #[must_use]
    pub fn setups_for(&self, op: &str) -> Vec<&SetupDecl> {
        self.setups.iter().filter(|s| s.operation == op).collect()
    }

    /// Resolve how an operation's receiver/parameters are supplied by setups,
    /// at the case level: a multi-step case shares one constructed receiver
    /// across all its step operations. The candidate pool is the union of
    /// setups linked to any operation the case runs. Returns the ordered setup
    /// bindings, or a precise error describing an unresolvable/ambiguous wiring.
    pub fn resolve_case(&self, ops: &[&str]) -> Result<Vec<SetupBinding>, String> {
        // Distinct operations, preserving first-seen order.
        let mut distinct: Vec<&str> = Vec::new();
        for &o in ops {
            if !distinct.contains(&o) {
                distinct.push(o);
            }
        }
        // Candidate setup pool for the whole case.
        let mut pool: Vec<&SetupDecl> = Vec::new();
        for o in &distinct {
            pool.extend(self.setups_for(o));
        }

        let mut bindings: Vec<SetupBinding> = Vec::new();
        let mut used = vec![false; pool.len()];
        let mut counter = 0usize;
        let new_var = |c: &mut usize| {
            let v = format!("__sg_setup{c}");
            *c += 1;
            v
        };

        // 1. Shared method receiver, from the first self-taking operation.
        let recv_ty = distinct
            .iter()
            .filter_map(|o| self.operations.get(*o))
            .find(|d| d.takes_self)
            .and_then(|d| d.method_of.clone());
        if let Some(recv_ty) = recv_ty {
            let cands: Vec<usize> = pool
                .iter()
                .enumerate()
                .filter(|(i, s)| !used[*i] && s.fills.is_none() && bare_type(&s.sig.return_type) == recv_ty)
                .map(|(i, _)| i)
                .collect();
            match cands.len() {
                0 => {
                    let op = distinct.first().copied().unwrap_or("");
                    return Err(format!(
                        "operation '{op}' is a method on '{recv_ty}' but no #[spec_setup(\"{op}\")] returns '{recv_ty}' to construct the receiver"
                    ));
                }
                1 => {
                    let i = cands[0];
                    used[i] = true;
                    bindings.push(SetupBinding {
                        fn_ident: pool[i].sig.fn_ident.clone(),
                        var: new_var(&mut counter),
                        params: pool[i].sig.params.clone(),
                        target: SetupTarget::Receiver,
                    });
                }
                n => {
                    return Err(format!(
                        "{n} setups return '{recv_ty}' for the receiver; a method receiver must be built by exactly one setup"
                    ));
                }
            }
        }

        // 2. Named parameters filled by setups (across all case operations).
        for o in &distinct {
            let Some(decl) = self.operations.get(*o) else { continue };
            for (p, ty) in &decl.sig.params {
                if bindings.iter().any(|b| matches!(&b.target, SetupTarget::Param(n) if n == p)) {
                    continue; // already bound by an earlier step
                }
                let bare_ty = bare_type(ty);
                let pinned: Vec<usize> = pool
                    .iter()
                    .enumerate()
                    .filter(|(i, s)| !used[*i] && s.fills.as_deref() == Some(p.as_str()) && bare_type(&s.sig.return_type) == bare_ty)
                    .map(|(i, _)| i)
                    .collect();
                if pinned.len() > 1 {
                    return Err(format!("operation '{o}': multiple setups fill parameter '{p}'"));
                }
                if let Some(&i) = pinned.first() {
                    used[i] = true;
                    bindings.push(SetupBinding {
                        fn_ident: pool[i].sig.fn_ident.clone(),
                        var: new_var(&mut counter),
                        params: pool[i].sig.params.clone(),
                        target: SetupTarget::Param(p.clone()),
                    });
                    continue;
                }
                let typed: Vec<usize> = pool
                    .iter()
                    .enumerate()
                    .filter(|(i, s)| !used[*i] && s.fills.is_none() && bare_type(&s.sig.return_type) == bare_ty)
                    .map(|(i, _)| i)
                    .collect();
                if typed.is_empty() {
                    continue; // supplied from inputs, not a setup
                }
                let same_type_params = decl.sig.params.iter().filter(|(_, t)| bare_type(t) == bare_ty).count();
                if typed.len() == 1 && same_type_params == 1 {
                    let i = typed[0];
                    used[i] = true;
                    bindings.push(SetupBinding {
                        fn_ident: pool[i].sig.fn_ident.clone(),
                        var: new_var(&mut counter),
                        params: pool[i].sig.params.clone(),
                        target: SetupTarget::Param(p.clone()),
                    });
                } else {
                    return Err(format!(
                        "operation '{o}' has {same_type_params} parameters of type '{bare_ty}' and {} setups producing it; pin each setup with fills = \"<param>\"",
                        typed.len()
                    ));
                }
            }
        }

        // 3. Leftover setups: side-effect calls, or report a bad `fills`.
        for (i, s) in pool.iter().enumerate() {
            if used[i] {
                continue;
            }
            if let Some(f) = &s.fills {
                let has_param = distinct
                    .iter()
                    .filter_map(|o| self.operations.get(*o))
                    .any(|d| d.sig.params.iter().any(|(p, _)| p == f));
                if has_param {
                    return Err(format!(
                        "setup fills '{f}' but its return type '{}' does not match parameter '{f}'",
                        s.sig.return_type.trim()
                    ));
                }
                return Err(format!("setup fills '{f}' but no operation in the case has a parameter '{f}'"));
            }
            bindings.push(SetupBinding {
                fn_ident: s.sig.fn_ident.clone(),
                var: new_var(&mut counter),
                params: s.sig.params.clone(),
                target: SetupTarget::SideEffect,
            });
        }

        Ok(bindings)
    }
}

/// How a setup's output is bound when running an operation.
#[derive(Debug, Clone)]
pub struct SetupBinding {
    pub fn_ident: String,
    pub var: String,
    pub params: Vec<(String, String)>,
    pub target: SetupTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupTarget {
    /// Fills the method `self` receiver.
    Receiver,
    /// Fills a named operation parameter.
    Param(String),
    /// Not consumed by the operation — called only for its side effects.
    SideEffect,
}

/// Strip references/whitespace to the bare type name for matching
/// (`&mut Account` → `Account`).
fn bare_type(ty: &str) -> String {
    ty.trim()
        .trim_start_matches('&')
        .trim_start()
        .trim_start_matches("mut ")
        .trim()
        .to_string()
}

/// A `#[spec_setup("operation", fills = "param")]` function.
#[derive(Debug, Clone)]
pub struct SetupDecl {
    pub sig: FnSig,
    /// The operation this setup prepares.
    pub operation: String,
    /// The operation parameter this setup fills, if pinned.
    pub fills: Option<String>,
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
    let mut setups: Vec<SetupDecl> = Vec::new();
    let mut operations = BTreeMap::new();
    let mut spec_event_structs = std::collections::BTreeSet::new();
    let mut spec_event_enums = std::collections::BTreeSet::new();

    // SpecEvent-derived structs/enums:
    //   #[derive(... SpecEvent ...)] ... struct|enum <NAME>
    for cap in find_iter(&src, "#[derive(") {
        let after = &src[cap..];
        let Some(close) = after.find(")]") else { continue };
        let derive_list = &after[9..close];
        if !derive_list.split(',').any(|t| t.trim() == "SpecEvent") {
            continue;
        }
        let rest = &after[close + 2..];
        if let Some((name, is_enum)) = scan_type_name(rest) {
            if is_enum {
                spec_event_enums.insert(name);
            } else {
                spec_event_structs.insert(name);
            }
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

    let mut pos = 0usize;
    let chars: Vec<char> = src.chars().collect();
    let total = chars.len();

    while pos < total {
        let ch = chars[pos];
        if ch == '{' {
            depth_to_impl_target.push(current_impl.last().cloned());
            current_impl.clear();
            pos += 1;
            continue;
        }
        if ch == '}' {
            depth_to_impl_target.pop();
            pos += 1;
            continue;
        }

        // Detect `impl <TYPE> {`
        if starts_word_at(&chars, pos, "impl") {
            let mut jj = pos + 4;
            while jj < total && chars[jj].is_whitespace() {
                jj += 1;
            }
            // Skip generics like <T>
            if jj < total && chars[jj] == '<' {
                let mut depth = 1;
                jj += 1;
                while jj < total && depth > 0 {
                    if chars[jj] == '<' {
                        depth += 1;
                    }
                    if chars[jj] == '>' {
                        depth -= 1;
                    }
                    jj += 1;
                }
                while jj < total && chars[jj].is_whitespace() {
                    jj += 1;
                }
            }
            // Read type name (single ident, optionally followed by generics).
            let start = jj;
            while jj < total && (chars[jj].is_alphanumeric() || chars[jj] == '_') {
                jj += 1;
            }
            let ty: String = chars[start..jj].iter().collect();
            // Skip optional generics on the type
            while jj < total && chars[jj].is_whitespace() {
                jj += 1;
            }
            if jj < total && chars[jj] == '<' {
                let mut depth = 1;
                jj += 1;
                while jj < total && depth > 0 {
                    if chars[jj] == '<' {
                        depth += 1;
                    }
                    if chars[jj] == '>' {
                        depth -= 1;
                    }
                    jj += 1;
                }
            }
            // The next `{` belongs to this impl.
            while jj < total && chars[jj] != '{' {
                jj += 1;
            }
            if jj < total && !ty.is_empty() {
                current_impl.clear();
                current_impl.push(ty);
            }
            pos = jj;
            continue;
        }

        // Detect attribute: #[spec_setup("NAME")] or #[spec_operation("NAME")]
        if ch == '#' && pos + 1 < total && chars[pos + 1] == '[' {
            // Find the closing ].
            let mut jj = pos + 2;
            let mut depth = 1;
            while jj < total && depth > 0 {
                if chars[jj] == '[' {
                    depth += 1;
                }
                if chars[jj] == ']' {
                    depth -= 1;
                }
                jj += 1;
            }
            let attr: String = chars[pos + 2..jj - 1].iter().collect();
            let attr_trim = attr.trim();
            let first_attr = parse_spec_attr(attr_trim);
            if let Some(first_attr) = first_attr {
                let mut pending: Vec<PendingAttr> = vec![first_attr];
                // Find the following `fn ident(...) -> ret`.
                let mut pp = jj;
                // Skip whitespace and other outer attributes / visibility,
                // collecting any additional spec attributes (stacked setups).
                loop {
                    while pp < total && chars[pp].is_whitespace() {
                        pp += 1;
                    }
                    if pp < total && chars[pp] == '#' {
                        // Parse the next attribute; collect it if it's a spec attr.
                        let mut dd = 1;
                        let mut qq = pp + 2;
                        while qq < total && dd > 0 {
                            if chars[qq] == '[' {
                                dd += 1;
                            }
                            if chars[qq] == ']' {
                                dd -= 1;
                            }
                            qq += 1;
                        }
                        let inner: String = chars[pp + 2..qq - 1].iter().collect();
                        if let Some(a) = parse_spec_attr(inner.trim()) {
                            pending.push(a);
                        }
                        pp = qq;
                        continue;
                    }
                    if starts_word_at(&chars, pp, "pub") {
                        pp += 3;
                        while pp < total && chars[pp].is_whitespace() {
                            pp += 1;
                        }
                        if pp < total && chars[pp] == '(' {
                            let mut dd = 1;
                            pp += 1;
                            while pp < total && dd > 0 {
                                if chars[pp] == '(' {
                                    dd += 1;
                                }
                                if chars[pp] == ')' {
                                    dd -= 1;
                                }
                                pp += 1;
                            }
                        }
                        continue;
                    }
                    // Skip leading function qualifiers (`async`, `const`, `unsafe`,
                    // `extern "C"`) so `#[spec_operation] async fn foo` is
                    // recognised.
                    if starts_word_at(&chars, pp, "async") || starts_word_at(&chars, pp, "const") || starts_word_at(&chars, pp, "unsafe") {
                        let word_len = if starts_word_at(&chars, pp, "async") || starts_word_at(&chars, pp, "const") {
                            5
                        } else {
                            6
                        };
                        pp += word_len;
                        continue;
                    }
                    if starts_word_at(&chars, pp, "extern") {
                        pp += 6;
                        while pp < total && chars[pp].is_whitespace() {
                            pp += 1;
                        }
                        // Optional ABI string.
                        if pp < total && chars[pp] == '"' {
                            pp += 1;
                            while pp < total && chars[pp] != '"' {
                                pp += 1;
                            }
                            if pp < total {
                                pp += 1;
                            }
                        }
                        continue;
                    }
                    break;
                }
                if !starts_word_at(&chars, pp, "fn") {
                    pos = jj;
                    continue;
                }
                pp += 2;
                while pp < total && chars[pp].is_whitespace() {
                    pp += 1;
                }
                let ident_start = pp;
                while pp < total && (chars[pp].is_alphanumeric() || chars[pp] == '_') {
                    pp += 1;
                }
                let fn_ident: String = chars[ident_start..pp].iter().collect();
                while pp < total && chars[pp] != '(' {
                    pp += 1;
                }
                if pp >= total {
                    pos = jj;
                    continue;
                }
                // Collect parens (balanced).
                let paren_start = pp + 1;
                let mut dd = 1;
                pp += 1;
                while pp < total && dd > 0 {
                    if chars[pp] == '(' {
                        dd += 1;
                    }
                    if chars[pp] == ')' {
                        dd -= 1;
                    }
                    pp += 1;
                }
                let paren_end = pp - 1;
                let param_text: String = chars[paren_start..paren_end].iter().collect();
                // Return type.
                let mut ret_text = String::new();
                while pp < total && chars[pp].is_whitespace() {
                    pp += 1;
                }
                if pp + 1 < total && chars[pp] == '-' && chars[pp + 1] == '>' {
                    pp += 2;
                    while pp < total && chars[pp].is_whitespace() {
                        pp += 1;
                    }
                    let r_start = pp;
                    let mut depth_ang = 0i32;
                    while pp < total {
                        let cc = chars[pp];
                        if cc == '<' {
                            depth_ang += 1;
                        }
                        if cc == '>' {
                            depth_ang -= 1;
                        }
                        if depth_ang <= 0 && (cc == '{' || cc == ';' || cc == 'w') {
                            // `w` for "where"
                            if cc == 'w' && !starts_word_at(&chars, pp, "where") {
                                pp += 1;
                                continue;
                            }
                            break;
                        }
                        pp += 1;
                    }
                    ret_text = chars[r_start..pp].iter().collect::<String>().trim().to_string();
                }

                let (params, takes_self) = parse_params(&param_text);
                let method_of = if takes_self {
                    depth_to_impl_target.last().cloned().flatten().filter(|s| !s.is_empty())
                } else {
                    None
                };
                let sig = FnSig {
                    fn_ident: fn_ident.clone(),
                    params,
                    return_type: ret_text,
                };
                for attr in pending {
                    match attr {
                        PendingAttr::Setup { op, fills } => {
                            setups.push(SetupDecl {
                                sig: sig.clone(),
                                operation: op,
                                fills,
                            });
                        }
                        PendingAttr::Op { name } => {
                            operations.insert(
                                name,
                                OpDecl {
                                    sig: sig.clone(),
                                    method_of: method_of.clone(),
                                    takes_self,
                                },
                            );
                        }
                    }
                }
                // Advance past the function signature so the outer loop does
                // not re-scan any stacked attributes we already consumed.
                pos = pp;
                continue;
            }
            pos = jj;
            continue;
        }

        pos += 1;
    }

    AnnotatedSource {
        setups,
        operations,
        spec_event_structs,
        spec_event_enums,
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

/// A pending `#[spec_setup]` / `#[spec_operation]` attribute collected while
/// scanning toward the function it annotates.
enum PendingAttr {
    Setup { op: String, fills: Option<String> },
    Op { name: String },
}

/// Parse a `spec_setup(...)` or `spec_operation(...)` attribute body.
fn parse_spec_attr(attr_trim: &str) -> Option<PendingAttr> {
    if let Some((op, fills)) = attr_setup(attr_trim) {
        Some(PendingAttr::Setup { op, fills })
    } else {
        attr_inner(attr_trim, "spec_operation").map(|name| PendingAttr::Op { name })
    }
}

/// Parse `spec_setup("operation"[, fills = "param"])` → (operation, fills).
fn attr_setup(attr: &str) -> Option<(String, Option<String>)> {
    let stripped = attr.strip_prefix("spec_setup")?;
    let stripped = stripped.trim_start().strip_prefix('(')?;
    let stripped = stripped.trim_start().strip_prefix('"')?;
    let end = stripped.find('"')?;
    let op = stripped[..end].to_string();
    let rest = &stripped[end + 1..];
    Some((op, parse_fills(rest)))
}

/// Parse a trailing `, fills = "param"` clause.
fn parse_fills(rest: &str) -> Option<String> {
    let r = rest.trim_start().strip_prefix(',')?;
    let r = r.trim_start().strip_prefix("fills")?;
    let r = r.trim_start().strip_prefix('=')?;
    let r = r.trim_start().strip_prefix('"')?;
    let end = r.find('"')?;
    Some(r[..end].to_string())
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

fn scan_type_name(rest: &str) -> Option<(String, bool)> {
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
    let (s, is_enum) = if let Some(r) = s.strip_prefix("struct") {
        (r, false)
    } else if let Some(r) = s.strip_prefix("enum") {
        (r, true)
    } else {
        return None;
    };
    let s = s.trim_start();
    let end = s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len());
    Some((s[..end].to_string(), is_enum))
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
