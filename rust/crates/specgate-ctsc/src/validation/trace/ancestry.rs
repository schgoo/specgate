use super::{BTreeMap, BTreeSet, TraceSpan, ValidationIssue, issue};

/// CTSC Trace Core run-span sentinel.
const RUN_NAME: &str = "conformance.run";
#[derive(Clone, Copy)]
enum Resolution<'a> {
    Root(&'a TraceSpan),
    SelfParent,
    Cycle,
    MissingRun,
    MultipleParents,
}

/// Validate that every non-run span resolves to one root run and each trace has one root.
///
/// Resolved suffixes are memoized, so shared deep ancestry is traversed only once.
pub(super) fn validate_ancestry<'a>(
    spans: &'a [TraceSpan],
    by_id: &BTreeMap<(&'a str, &'a str), Vec<&'a TraceSpan>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut resolved = Vec::with_capacity(spans.len());
    // Ordered diagnostics require tree collections; BTreeMap/BTreeSet expose no capacity constructor.
    let mut trace_roots = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut cache = BTreeMap::new();
    let mut path = Vec::with_capacity(spans.len());
    let mut seen = BTreeSet::new();
    for span in spans.iter().filter(|span| span.name != RUN_NAME) {
        match run_ancestor(span, by_id, &mut cache, &mut path, &mut seen) {
            Resolution::Root(run) => {
                trace_roots.entry(span.trace_id.as_str()).or_default().insert(run.span_id.as_str());
                resolved.push((span, run.span_id.as_str()));
            }
            Resolution::SelfParent => issue(span.location.clone(), "CTSC ancestor chain contains a self-parent cycle", issues),
            Resolution::Cycle => issue(span.location.clone(), "CTSC ancestor chain contains a cycle", issues),
            Resolution::MissingRun => issue(
                span.location.clone(),
                "CTSC ancestor chain must terminate at a conformance.run span",
                issues,
            ),
            Resolution::MultipleParents => issue(
                span.location.clone(),
                "CTSC ancestor chain has invalid multiple ancestry because a parent span ID resolves to multiple CTSC spans",
                issues,
            ),
        }
    }
    for (span, _) in resolved {
        if trace_roots.get(span.trace_id.as_str()).is_some_and(|roots| roots.len() > 1) {
            issue(
                span.location.clone(),
                "CTSC spans sharing a traceId must terminate at the same conformance.run span",
                issues,
            );
        }
    }
}

/// Resolve a span's unique run ancestor, reusing cached ancestry suffixes.
fn run_ancestor<'a>(
    span: &'a TraceSpan,
    by_id: &BTreeMap<(&'a str, &'a str), Vec<&'a TraceSpan>>,
    cache: &mut BTreeMap<(&'a str, &'a str), Resolution<'a>>,
    path: &mut Vec<(&'a str, &'a str)>,
    seen: &mut BTreeSet<(&'a str, &'a str)>,
) -> Resolution<'a> {
    let mut current = span;
    path.clear();
    seen.clear();
    let resolution = loop {
        let key = (current.trace_id.as_str(), current.span_id.as_str());
        if let Some(cached) = cache.get(&key).copied() {
            break cached;
        }
        if !seen.insert(key) {
            break Resolution::Cycle;
        }
        path.push(key);
        if current.name == RUN_NAME {
            break if current.parent_span_id.is_empty() {
                Resolution::Root(current)
            } else {
                Resolution::MissingRun
            };
        }
        if current.parent_span_id == current.span_id {
            break Resolution::SelfParent;
        }
        if current.parent_span_id.is_empty() {
            break Resolution::MissingRun;
        }
        let Some(parents) = by_id.get(&(current.trace_id.as_str(), current.parent_span_id.as_str())) else {
            break Resolution::MissingRun;
        };
        let [parent] = parents.as_slice() else {
            break Resolution::MultipleParents;
        };
        current = parent;
    };
    for key in path.iter().copied() {
        cache.insert(key, resolution);
    }
    resolution
}
