//! Assertion matcher.
//!
//! Top-level: walks `expected` and `actual` together. Plain Event/Run
//! assertions form an ordered subsequence (gaps allowed). `$unordered`
//! blocks match all of their items in any order, each as a subsequence
//! starting from the current cursor, and advance the cursor past the
//! furthest match. `$anywhere` blocks match each item anywhere in the
//! whole trace and do not advance the ordered cursor.

use crate::types::{AnyArg, AssertValue, Assertion, Matcher, TraceEvent, Value};
use std::cmp::Ordering;

pub fn matches(expected: &[Assertion], actual: &[TraceEvent]) -> bool {
    match_ordered(expected, actual, 0).is_some()
}

fn match_ordered(expected: &[Assertion], actual: &[TraceEvent], mut cursor: usize) -> Option<usize> {
    for a in expected {
        cursor = match a {
            Assertion::Event { .. } | Assertion::Run { .. } => find_leaf(a, actual, cursor)? + 1,
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
    if let Assertion::Event {
        name,
        value: AssertValue::Matcher(Matcher::Exists(present)),
    } = a
    {
        let any = actual.iter().any(|ev| match ev {
            TraceEvent::Event { name: en, .. } => en == name,
            TraceEvent::Run { .. } => false,
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
        (Assertion::Event { name, value }, TraceEvent::Event { name: en, value: ev_val }) => {
            if name != en {
                return false;
            }
            match value {
                AssertValue::Exact(v) => values_equal(v, ev_val),
                AssertValue::Matcher(m) => matcher_matches(m, ev_val),
            }
        }
        (Assertion::Run { operation }, TraceEvent::Run { operation: actual_op }) => operation == actual_op,
        _ => false,
    }
}

/// Compare two `Value`s with the harness's slightly relaxed equality:
/// Integer/Float coerce when compared to String numerics (so YAML `value: 4`
/// matches a trace `Value::Integer(4)`).
fn values_equal(expected: &Value, actual: &Value) -> bool {
    if expected == actual {
        return true;
    }
    match (expected, actual) {
        (Value::String(s), other) | (other, Value::String(s)) => string_matches_scalar(s, other),
        (Value::List(a), Value::List(b)) => a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y)),
        (Value::List(a), Value::Set(b)) | (Value::Set(b), Value::List(a)) => {
            a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| values_equal(x, y)))
        }
        (Value::Map(a), Value::Map(b)) => {
            // Map matching is subset-based on the expected side: every key
            // in `expected` must be present in `actual` with a matching value.
            // Extra keys in `actual` are allowed. This matches the spec's
            // `map_subset_match` semantics where an asserted partial map
            // passes against a fuller actual map.
            a.iter().all(|(k, v)| b.get(k).is_some_and(|bv| values_equal(v, bv)))
        }
        _ => false,
    }
}

/// True if `s` is the string form of `actual` (used to make `value: "5"`
/// match a trace `Value::Integer(5)`).
fn string_matches_scalar(s: &str, actual: &Value) -> bool {
    match actual {
        Value::String(a) => a == s,
        Value::Integer(i) => i.to_string() == s,
        Value::Float(f) => f.to_string() == s,
        Value::Bool(b) => b.to_string() == s,
        _ => false,
    }
}

fn matcher_matches(m: &Matcher, v: &Value) -> bool {
    match m {
        Matcher::Eq(target) => values_equal(target, v),
        Matcher::Size(n) => length_of(v).is_some_and(|l| l == *n),
        Matcher::Contains(arg) => match v {
            Value::List(xs) => xs.iter().any(|x| match arg.as_ref() {
                AnyArg::Value(val) => values_equal(val, x),
                AnyArg::Matcher(m) => matcher_matches(m, x),
            }),
            Value::Set(xs) => xs.iter().any(|x| match arg.as_ref() {
                AnyArg::Value(val) => values_equal(val, x),
                AnyArg::Matcher(m) => matcher_matches(m, x),
            }),
            Value::String(s) => match arg.as_ref() {
                AnyArg::Value(Value::String(needle)) => s.contains(needle.as_str()),
                _ => false,
            },
            _ => false,
        },
        Matcher::ContainsAll(items) => items
            .iter()
            .all(|it| matcher_matches(&Matcher::Contains(Box::new(AnyArg::Value(it.clone()))), v)),
        Matcher::Excludes(items) => items
            .iter()
            .all(|it| !matcher_matches(&Matcher::Contains(Box::new(AnyArg::Value(it.clone()))), v)),
        Matcher::Match(spec) => match v {
            Value::Map(m) => spec.iter().all(|(k, val)| m.get(k).is_some_and(|av| values_equal(val, av))),
            _ => false,
        },
        Matcher::Exists(_) => true, // handled at find_leaf level
        Matcher::Any(arg) => {
            let items: Vec<&Value> = match v {
                Value::List(xs) => xs.iter().collect(),
                Value::Set(xs) => xs.iter().collect(),
                _ => return false,
            };
            items.iter().any(|x| match arg.as_ref() {
                AnyArg::Value(val) => values_equal(val, x),
                AnyArg::Matcher(m) => matcher_matches(m, x),
            })
        }
        Matcher::Every(arg) => {
            let items: Vec<&Value> = match v {
                Value::List(xs) => xs.iter().collect(),
                Value::Set(xs) => xs.iter().collect(),
                _ => return false,
            };
            if items.is_empty() {
                return true;
            }
            items.iter().all(|x| match arg.as_ref() {
                AnyArg::Value(val) => values_equal(val, x),
                AnyArg::Matcher(m) => matcher_matches(m, x),
            })
        }
        Matcher::Not(inner) => !matcher_matches(inner, v),
        Matcher::Gt(target) => numeric_compare(v, target).is_some_and(|o| o == Ordering::Greater),
        Matcher::Gte(target) => numeric_compare(v, target).is_some_and(|o| matches!(o, Ordering::Greater | Ordering::Equal)),
        Matcher::Lt(target) => numeric_compare(v, target).is_some_and(|o| o == Ordering::Less),
        Matcher::Lte(target) => numeric_compare(v, target).is_some_and(|o| matches!(o, Ordering::Less | Ordering::Equal)),
        Matcher::Type(t) => v.type_name() == t.as_str() || (t == "int" && matches!(v, Value::Integer(_))),
        Matcher::Matches(pat) => match v {
            Value::String(s) => regex::Regex::new(pat).is_ok_and(|r| r.is_match(s)),
            _ => false,
        },
        Matcher::Composite(parts) => parts.iter().all(|p| matcher_matches(p, v)),
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

fn numeric_compare(v: &Value, target: &Value) -> Option<Ordering> {
    match (v, target) {
        (Value::Integer(a), Value::Integer(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        #[allow(clippy::cast_precision_loss)] // i64→f64 comparison: precision loss is acceptable for ordering
        (Value::Integer(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        #[allow(clippy::cast_precision_loss)] // i64→f64 comparison: precision loss is acceptable for ordering
        (Value::Float(a), Value::Integer(b)) => a.partial_cmp(&(*b as f64)),
        _ => None,
    }
}

fn match_unordered(items: &[Assertion], actual: &[TraceEvent], cursor: usize) -> Option<usize> {
    let n = items.len();
    let mut assignment: Vec<Option<usize>> = vec![None; n];
    if assign_unordered(items, actual, cursor, &mut assignment, 0) {
        let max = assignment.iter().filter_map(|x| *x).max();
        Some(max.map_or(cursor, |m| m + 1))
    } else {
        None
    }
}

fn assign_unordered(items: &[Assertion], actual: &[TraceEvent], cursor: usize, assignment: &mut [Option<usize>], idx: usize) -> bool {
    if idx == items.len() {
        return true;
    }
    let item = &items[idx];
    match item {
        Assertion::Event { .. } | Assertion::Run { .. } => {
            for i in cursor..actual.len() {
                if assignment.contains(&Some(i)) {
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
            Assertion::Anywhere { items: sub } | Assertion::Unordered { items: sub } => {
                if !match_anywhere(sub, actual) {
                    return false;
                }
            }
        }
    }
    true
}
