//! An async operation constructed inside one operation and resumed on another
//! thread.
//!
//! The capture evidence is the parent: `fetch` is built while `migrate` is the
//! current operation, handed to a worker thread, and polled to completion
//! there. Its span must still be a child of `migrate`, which it can only be if
//! the parent was captured where the future was constructed.
//!
//! No `spec_trace!` observation is emitted here. Discovery has no link-time
//! observation metadata yet, so a captured observation fails linked validation
//! (`component/fixture.fallible_unit`'s `observation-not-declared`). Observation
//! attribution across the handoff is pinned by
//! `rust/crates/specgate/tests/native_capture.rs` instead.

use specgate::spec_operation;

#[spec_operation("fetch", spec = "fixture.async_migration")]
pub async fn fetch(url: String) -> String {
    std::future::ready(()).await;
    format!("response from {url}")
}

#[spec_operation("migrate", spec = "fixture.async_migration")]
pub async fn migrate(url: String) -> String {
    let migrating = fetch(url);
    let worker = std::thread::spawn(move || smol::block_on(migrating));
    std::future::ready(()).await;
    worker
        .join()
        .expect("the migrated future ran to completion on its worker thread")
}

/// Exactly one thread polls at a time: the calling thread blocks in `join`
/// while the worker drives the migrated future, so span IDs and the logical
/// clock stay deterministic. A worker pool would make poll order — and
/// therefore the golden — nondeterministic.
#[test]
fn a_migrated_future_is_recorded_under_the_operation_that_built_it() {
    assert_eq!(
        smol::block_on(migrate("https://example.test".to_string())),
        "response from https://example.test"
    );
}
