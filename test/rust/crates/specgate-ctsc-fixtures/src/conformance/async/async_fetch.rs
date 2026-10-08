use specgate::spec_operation;

#[spec_operation("fetch", spec = "fixture.async_fetch")]
pub async fn fetch(url: &str) -> String {
    std::future::ready(format!("response from {url}")).await
}

#[test]
fn async_operation_is_discovery_valid_without_native_capture() {
    let operation = specgate::__rt::SPECGATE_OPS
        .iter()
        .find(|operation| {
            operation.component().as_str() == "fixture.async_fetch"
                && operation.name().as_str() == "fetch"
        })
        .expect("async fetch metadata");
    assert!(operation.is_async());
    let [url] = operation.params() else {
        panic!("async fetch must have exactly one parameter");
    };
    assert_eq!(url.name(), "url");
    assert_eq!(url.rust_type(), "& str");
}

/// Drive the operation to completion on the calling thread so native capture
/// records exactly one span for it.
///
/// Deliberately a bare `block_on`: no timeout, no `select!`, no spawn, and no
/// multi-threaded executor. A future that is polled and then dropped while
/// still `Pending` is abandoned, and abandonment is an accepted but
/// unimplemented terminal state (`docs/decisions/abandonment-terminal-state.md`).
#[test]
fn directly_awaited_async_operation_runs_to_completion() {
    assert_eq!(
        smol::block_on(fetch("https://example.test")),
        "response from https://example.test"
    );
}
