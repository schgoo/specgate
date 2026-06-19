//! Assertion matcher.
//!
//! Top-level: walks `expected` and `actual` together. Plain Event/Run
//! assertions form an ordered subsequence (gaps allowed). `$unordered`
//! blocks match all of their items in any order, each as a subsequence
//! starting from the current cursor, and advance the cursor past the
//! furthest match. `$anywhere` blocks match each item anywhere in the
//! whole trace and do not advance the ordered cursor.

use crate::types::{AnyArg, AssertValue, Assertion, Matcher, TraceEvent, Value};

pub fn matches(expected: &[Assertion], actual: &[TraceEvent]) -> bool {
    match_ordered(expected, actual, 0).is_some()
}

fn match_ordered(expected: &[Assertion], actual: &[TraceEvent], mut cursor: usize) -> Option<usize> {
    for a in expected {
        cursor = match a {
            Assertion::Event { .. } | Assertion::Run { .. } => {
                find_leaf(a, actual, cursor)? + 1
            }
            Assertion::Unordered { items } => match_unordered(items, actual, cursor)?,
            Assertion::Anywhere { items } => {
                if !match_anywhere(items, actual) {
                    return None;
                }
                cursor
            }
        };
    }
    Some(cursor)
}

fn find_leaf(a: &Assertion, actual: &[TraceEvent], start: usize) -> Option<usize> {
    // Special-case $exists: scan the whole stream.
    if let Assertion::Event { name, value: AssertValue::Matcher(Matcher::Exists(present)) } = a {
        let any = actual.iter().any(|ev| match ev {
            TraceEvent::Event { name: en, .. } => en == name,
            _ => false,
        });
        if any == *present {
            // Anchor at `start` so the cursor advances minimally.
            return Some(start.saturating_sub(1).max(start));
        }
        return None;
    }
    for (i, ev) in actual.iter().enumerate().skip(start) {
        if leaf_matches(a, ev) {
            return Some(i);
        }
    }
    None
}

fn leaf_matches(a: &Assertion, ev: &TraceEvent) -> bool {
    match (a, ev) {
        (
            Assertion::Event { name, value },
            TraceEvent::Event { name: en, value: ev_val },
        ) => {
            if name != en { return false; }
            match value {
                AssertValue::Exact(v) => values_equal(v, ev_val),
                AssertValue::Matcher(m) => matcher_matches(m, ev_val),
            }
        }
        (Assertion::Run { operation }, TraceEvent::Run { operation: actual_op }) => {
            operation == actual_op
        }
        _ => false,
    }
}

/// Compare two `Value`s with the harness's slightly relaxed equality:
/// Integer/Float coerce when compared to String numerics (so YAML `value: 4`
/// matches a trace `Value::Integer(4)`).
fn values_equal(expected: &Value, actual: &Value) -> bool {
    if expected == actual { return true; }
    match (expected, actual) {
        (Value::String(s), other) => string_matches_scalar(s, other),
        (other, Value::String(s)) => string_matches_scalar(s, other),
        (Value::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        (Value::List(a), Value::Set(b)) | (Value::Set(b), Value::List(a)) => {
            a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| values_equal(x, y)))
        }
        (Value::Map(a), Value::Map(b)) => {
            // Map matching is subset-based on the expected side: every key
            // in `expected` must be present in `actual` with a matching value.
            // Extra keys in `actual` are allowed. This matches the spec's
            // `map_subset_match` semantics where an asserted partial map
            // passes against a fuller actual map.
            a.iter().all(|(k, v)| b.get(k).map(|bv| values_equal(v, bv)).unwrap_or(false))
        }
        _ => false,
    }
}

/// True if `s` is the string form of `actual` (used to make `value: "5"`
/// match a trace `Value::Integer(5)`).
fn string_matches_scalar(s: &str, actual: &Value) -> bool {
    match actual {
        Value::String(a) => a == s,
        Value::Integer(i) => &i.to_string() == s,
        Value::Float(f) => &f.to_string() == s,
        Value::Bool(b) => &b.to_string() == s,
        _ => false,
    }
}

fn matcher_matches(m: &Matcher, v: &Value) -> bool {
    match m {
        Matcher::Eq(target) => values_equal(target, v),
        Matcher::Size(n) => length_of(v).map(|l| l == *n).unwrap_or(false),
        Matcher::Contains(arg) => contains_arg(arg, v),
        Matcher::ContainsAll(items) => items.iter().all(|it| contains_arg(it, v)),
        Matcher::Excludes(items) => items.iter().all(|it| !contains_arg(it, v)),
        Matcher::Match(spec) => match v {
            Value::Map(m) => spec
                .iter()
                .all(|(k, val)| m.get(k).map(|av| values_equal(val, av)).unwrap_or(false)),
            _ => false,
        },
        Matcher::Exists(_) => true, // handled at find_leaf level
        Matcher::Any(arg) => {
            let items: Vec<&Value> = match v {
                Value::List(xs) => xs.iter().collect(),
                Value::Set(xs) => xs.iter().collect(),
                _ => return false,
            };
            items.iter().any(|x| arg_matches(arg, x))
        }
        Matcher::Every(arg) => {
            let items: Vec<&Value> = match v {
                Value::List(xs) => xs.iter().collect(),
                Value::Set(xs) => xs.iter().collect(),
                _ => return false,
            };
            items.iter().all(|x| arg_matches(arg, x))
        }
        Matcher::Not(arg) => !arg_matches(arg, v),
        Matcher::Gt(target) => numeric_cmp(v, target).map(|o| o == std::cmp::Ordering::Greater).unwrap_or(false),
        Matcher::Gte(target) => numeric_cmp(v, target).map(|o| o != std::cmp::Ordering::Less).unwrap_or(false),
        Matcher::Lt(target) => numeric_cmp(v, target).map(|o| o == std::cmp::Ordering::Less).unwrap_or(false),
        Matcher::Lte(target) => numeric_cmp(v, target).map(|o| o != std::cmp::Ordering::Greater).unwrap_or(false),
        Matcher::Type(t) => type_matches(t, v),
        Matcher::Matches(pat) => match v {
            Value::String(s) => regex_match(pat, s),
            _ => false,
        },
        Matcher::Composite(parts) => parts.iter().all(|p| matcher_matches(p, v)),
    }
}

fn arg_matches(arg: &AnyArg, v: &Value) -> bool {
    match arg {
        AnyArg::Value(val) => values_equal(val, v),
        AnyArg::Matcher(m) => matcher_matches(m, v),
    }
}

/// `$contains` semantics: the argument matches one of the collection's
/// elements (deep-eq or matcher), or for strings, is a substring.
fn contains_arg(arg: &AnyArg, v: &Value) -> bool {
    match v {
        Value::List(xs) => xs.iter().any(|x| arg_matches(arg, x)),
        Value::Set(xs) => xs.iter().any(|x| arg_matches(arg, x)),
        Value::Map(map) => map.values().any(|x| arg_matches(arg, x)),
        Value::String(s) => match arg {
            AnyArg::Value(Value::String(needle)) => s.contains(needle.as_str()),
            _ => false,
        },
        _ => false,
    }
}

/// Compare `v` to `target` numerically. Returns `Some(ordering)` when both
/// are numeric (coercing Integer/Float as needed); `None` otherwise.
fn numeric_cmp(v: &Value, target: &Value) -> Option<std::cmp::Ordering> {
    let a = numeric_value(v)?;
    let b = numeric_value(target)?;
    a.partial_cmp(&b)
}

fn numeric_value(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn type_matches(t: &str, v: &Value) -> bool {
    let actual = v.type_name();
    match t {
        "string" => matches!(v, Value::String(_)),
        "number" | "int" => matches!(v, Value::Integer(_)) || (t == "number" && matches!(v, Value::Float(_))),
        "float" => matches!(v, Value::Float(_)),
        "bool" => matches!(v, Value::Bool(_)),
        "list" => matches!(v, Value::List(_)),
        "map" => matches!(v, Value::Map(_)),
        "set" => matches!(v, Value::Set(_)),
        "null" => matches!(v, Value::String(s) if s.is_empty()),
        other => other == actual,
    }
}

fn length_of(v: &Value) -> Option<usize> {
    match v {
        Value::List(xs) => Some(xs.len()),
        Value::Set(xs) => Some(xs.len()),
        Value::Map(xs) => Some(xs.len()),
        Value::String(s) => Some(s.chars().count()),
        _ => None,
    }
}

/// Regex matcher supporting `^`, `$`, char classes (`[A-Z]`, `\d`, `\w`,
/// `\s`, `[abc]`, `[^abc]`), `.`, and quantifiers `+`, `*`, `?`. Sufficient
/// for the regex shapes used by spec fixtures; we don't ship a full engine
/// because no regex crate is available in the offline sandbox.
fn regex_match(pat: &str, s: &str) -> bool {
    let tokens = compile_regex(pat);
    let anchored_start = pat.starts_with('^');
    let anchored_end = pat.ends_with('$') && !pat.ends_with("\\$");
    if anchored_start {
        return match_tokens(&tokens, 0, s, anchored_end).is_some();
    }
    for start in 0..=s.len() {
        if !s.is_char_boundary(start) { continue; }
        if match_tokens(&tokens, 0, &s[start..], anchored_end).is_some() {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone)]
enum RxTok {
    /// Single character class with quantifier. `Many` is "one or more",
    /// `Star` is zero or more, `Opt` is zero or one, `One` is exactly one.
    Class(CharClass, Quant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quant { One, Many, Star, Opt }

#[derive(Debug, Clone)]
enum CharClass {
    Any,
    Literal(char),
    Digit,
    Word,
    Space,
    Set(Vec<(char, char)>, bool), // ranges, negated?
}

impl CharClass {
    fn matches(&self, c: char) -> bool {
        match self {
            CharClass::Any => c != '\n',
            CharClass::Literal(l) => *l == c,
            CharClass::Digit => c.is_ascii_digit(),
            CharClass::Word => c.is_ascii_alphanumeric() || c == '_',
            CharClass::Space => c.is_whitespace(),
            CharClass::Set(ranges, neg) => {
                let in_set = ranges.iter().any(|(lo, hi)| c >= *lo && c <= *hi);
                in_set ^ *neg
            }
        }
    }
}

fn compile_regex(pat: &str) -> Vec<RxTok> {
    // Strip ^/$ anchors before tokenising — they're handled by callers.
    let mut body = pat;
    if body.starts_with('^') { body = &body[1..]; }
    if body.ends_with('$') && !body.ends_with("\\$") { body = &body[..body.len()-1]; }
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (class, consumed) = parse_class(&chars, i);
        i += consumed;
        let quant = match chars.get(i) {
            Some('+') => { i += 1; Quant::Many }
            Some('*') => { i += 1; Quant::Star }
            Some('?') => { i += 1; Quant::Opt }
            _ => Quant::One,
        };
        out.push(RxTok::Class(class, quant));
    }
    out
}

fn parse_class(chars: &[char], i: usize) -> (CharClass, usize) {
    match chars[i] {
        '.' => (CharClass::Any, 1),
        '\\' => {
            let c = chars.get(i + 1).copied().unwrap_or('\\');
            let cl = match c {
                'd' => CharClass::Digit,
                'w' => CharClass::Word,
                's' => CharClass::Space,
                other => CharClass::Literal(other),
            };
            (cl, 2)
        }
        '[' => {
            let mut j = i + 1;
            let neg = chars.get(j).copied() == Some('^');
            if neg { j += 1; }
            let mut ranges: Vec<(char, char)> = Vec::new();
            while j < chars.len() && chars[j] != ']' {
                let lo = if chars[j] == '\\' {
                    j += 1;
                    chars.get(j).copied().unwrap_or('\\')
                } else { chars[j] };
                j += 1;
                if chars.get(j).copied() == Some('-') && chars.get(j + 1).is_some_and(|&c| c != ']') {
                    j += 1;
                    let hi = if chars[j] == '\\' {
                        j += 1;
                        chars.get(j).copied().unwrap_or('\\')
                    } else { chars[j] };
                    j += 1;
                    ranges.push((lo, hi));
                } else {
                    ranges.push((lo, lo));
                }
            }
            if j < chars.len() && chars[j] == ']' { j += 1; }
            (CharClass::Set(ranges, neg), j - i)
        }
        c => (CharClass::Literal(c), 1),
    }
}

fn match_tokens(toks: &[RxTok], ti: usize, s: &str, anchored_end: bool) -> Option<usize> {
    if ti >= toks.len() {
        if anchored_end && !s.is_empty() {
            return None;
        }
        return Some(0);
    }
    let RxTok::Class(class, quant) = &toks[ti];
    match quant {
        Quant::One => {
            let mut it = s.chars();
            let c = it.next()?;
            if !class.matches(c) { return None; }
            let rest = it.as_str();
            match_tokens(toks, ti + 1, rest, anchored_end).map(|n| n + (s.len() - rest.len()))
        }
        Quant::Opt => {
            if let Some(c) = s.chars().next() {
                if class.matches(c) {
                    let rest_len = c.len_utf8();
                    if let Some(n) = match_tokens(toks, ti + 1, &s[rest_len..], anchored_end) {
                        return Some(n + rest_len);
                    }
                }
            }
            match_tokens(toks, ti + 1, s, anchored_end)
        }
        Quant::Star | Quant::Many => {
            // Greedy: consume as many as possible, then backtrack.
            let mut idx = 0usize;
            let mut positions = vec![0usize];
            for c in s.chars() {
                if !class.matches(c) { break; }
                idx += c.len_utf8();
                positions.push(idx);
            }
            let min = if matches!(quant, Quant::Many) { 1 } else { 0 };
            for k in (min..positions.len()).rev() {
                let consumed = positions[k];
                if let Some(n) = match_tokens(toks, ti + 1, &s[consumed..], anchored_end) {
                    return Some(n + consumed);
                }
            }
            None
        }
    }
}

fn match_unordered(items: &[Assertion], actual: &[TraceEvent], cursor: usize) -> Option<usize> {
    let n = items.len();
    let mut assignment: Vec<Option<usize>> = vec![None; n];
    if assign_unordered(items, actual, cursor, &mut assignment, 0) {
        let max = assignment.iter().filter_map(|x| *x).max();
        Some(max.map(|m| m + 1).unwrap_or(cursor))
    } else {
        None
    }
}

fn assign_unordered(
    items: &[Assertion],
    actual: &[TraceEvent],
    cursor: usize,
    assignment: &mut [Option<usize>],
    idx: usize,
) -> bool {
    if idx == items.len() {
        return true;
    }
    let item = &items[idx];
    match item {
        Assertion::Event { .. } | Assertion::Run { .. } => {
            for i in cursor..actual.len() {
                if assignment.iter().any(|a| *a == Some(i)) {
                    continue;
                }
                if leaf_matches(item, &actual[i]) {
                    assignment[idx] = Some(i);
                    if assign_unordered(items, actual, cursor, assignment, idx + 1) {
                        return true;
                    }
                    assignment[idx] = None;
                }
            }
            false
        }
        Assertion::Anywhere { items: sub } => {
            if !match_anywhere(sub, actual) {
                return false;
            }
            assign_unordered(items, actual, cursor, assignment, idx + 1)
        }
        Assertion::Unordered { items: sub } => {
            let mut combined = items[..idx].to_vec();
            combined.extend(sub.iter().cloned());
            combined.extend(items[idx + 1..].iter().cloned());
            match_unordered(&combined, actual, cursor).is_some()
        }
    }
}

fn match_anywhere(items: &[Assertion], actual: &[TraceEvent]) -> bool {
    for it in items {
        match it {
            Assertion::Event { .. } | Assertion::Run { .. } => {
                if find_leaf(it, actual, 0).is_none() {
                    return false;
                }
            }
            Assertion::Anywhere { items: sub } => {
                if !match_anywhere(sub, actual) {
                    return false;
                }
            }
            Assertion::Unordered { items: sub } => {
                if !match_anywhere(sub, actual) {
                    return false;
                }
            }
        }
    }
    true
}
