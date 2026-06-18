//! Subsequence matcher.
//!
//! Walks `expected` and `actual` together; passes iff every expected
//! entry appears in `actual` in order. Each expected entry is a
//! single-key map: `{run: <name>}` matches a `Run`, otherwise matches
//! an `Event` whose name and stringified value equal the entry.

use crate::types::TraceEvent;
use std::collections::BTreeMap;

pub fn matches(expected: &[BTreeMap<String, String>], actual: &[TraceEvent]) -> bool {
    let mut i = 0;
    for exp in expected {
        loop {
            if i >= actual.len() {
                return false;
            }
            if entry_matches(exp, &actual[i]) {
                i += 1;
                break;
            }
            i += 1;
        }
    }
    true
}

fn entry_matches(exp: &BTreeMap<String, String>, ev: &TraceEvent) -> bool {
    if exp.len() != 1 {
        return false;
    }
    let (k, v) = exp.iter().next().unwrap();
    if k == "run" {
        matches!(ev, TraceEvent::Run { operation } if operation == v)
    } else {
        matches!(ev, TraceEvent::Event { name, value } if name == k && value == v)
    }
}
