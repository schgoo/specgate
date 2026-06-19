//! Assertion matcher.
//!
//! Top-level: walks `expected` and `actual` together. Plain Event/Run
//! assertions form an ordered subsequence (gaps allowed). `$unordered`
//! blocks match all of their items in any order, each as a subsequence
//! starting from the current cursor, and advance the cursor past the
//! furthest match. `$anywhere` blocks match each item anywhere in the
//! whole trace and do not advance the ordered cursor.

use crate::types::{Assertion, TraceEvent};

pub fn matches(expected: &[Assertion], actual: &[TraceEvent]) -> bool {
    match_ordered(expected, actual, 0).is_some()
}

/// Walk an ordered assertion list starting at `cursor`. Returns the
/// position after the last consumed event on success.
fn match_ordered(expected: &[Assertion], actual: &[TraceEvent], mut cursor: usize) -> Option<usize> {
    for a in expected {
        cursor = match a {
            Assertion::Event { .. } | Assertion::Run { .. } => {
                find_leaf(a, actual, cursor)? + 1
            }
            Assertion::Unordered { items } => match_unordered(items, actual, cursor)?,
            Assertion::Anywhere { items } => {
                // $anywhere doesn't advance the ordered cursor.
                if !match_anywhere(items, actual) {
                    return None;
                }
                cursor
            }
        };
    }
    Some(cursor)
}

/// Find the first index `>= start` where the leaf assertion matches an event.
fn find_leaf(a: &Assertion, actual: &[TraceEvent], start: usize) -> Option<usize> {
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
            TraceEvent::Event {
                name: en,
                value: ev,
            },
        ) => name == en && value == ev,
        (Assertion::Run { operation }, TraceEvent::Run { operation: actual_op }) => {
            operation == actual_op
        }
        _ => false,
    }
}

/// $unordered: try every permutation-style assignment of items to positions
/// `>= cursor` such that no two items map to the same position. Returns the
/// position just past the furthest matched index. Items inside $unordered may
/// themselves be leaf assertions or nested directives.
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
            // Nested $unordered: just splice items in.
            let mut combined = items[..idx].to_vec();
            combined.extend(sub.iter().cloned());
            combined.extend(items[idx + 1..].iter().cloned());
            match_unordered(&combined, actual, cursor).is_some()
        }
    }
}

/// $anywhere: every item must appear somewhere in the trace. Items don't
/// have to be in any order or position, but each item is matched
/// independently.
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
                // $unordered nested inside $anywhere acts the same as $anywhere
                // for the sub-items (since position is irrelevant).
                if !match_anywhere(sub, actual) {
                    return false;
                }
            }
        }
    }
    true
}
