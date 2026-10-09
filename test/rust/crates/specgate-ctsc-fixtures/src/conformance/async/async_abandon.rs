//! An annotated async operation that is polled once and then dropped.
//!
//! Dropping an in-flight instrumented future is not a fault and not a
//! completion: the operation never reached an outcome. The producer records
//! `conformance.abandoned` on the inner span and leaves its status `UNSET`, and
//! the enclosing `abandon` operation still completes normally — abandonment
//! does not propagate.
//!
//! The schedule is written out by hand rather than handed to an executor. One
//! poll, an observed `Pending`, then a drop: no `select!`, no worker pool and no
//! timers, so the synthesized span IDs and the logical clock are deterministic
//! and the golden cannot go flaky.
//!
//! No `spec_trace!` observation is emitted here. Discovery has no link-time
//! observation metadata yet, so a captured observation fails linked validation
//! (see `component/fixture.fallible_unit`'s `observation-not-declared`).

use specgate::spec_operation;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Never resolves, so the only way out of this operation is to be dropped.
#[spec_operation("work", spec = "fixture.async_abandon")]
pub async fn work(value: i32) -> i32 {
    Suspend.await;
    value * 2
}

/// Polls `work` exactly once, observes `Pending`, and drops it.
#[spec_operation("abandon", spec = "fixture.async_abandon")]
pub async fn abandon(value: i32) -> i32 {
    let mut context = Context::from_waker(std::task::Waker::noop());
    let mut abandoned = Box::pin(work(value));

    assert!(
        abandoned.as_mut().poll(&mut context).is_pending(),
        "the inner operation must open before it is abandoned"
    );
    drop(abandoned);
    value + 1
}

/// Always pending, and never wakes: polling it again would be a deadlock, which
/// is precisely why the caller drops it instead.
struct Suspend;

impl Future for Suspend {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<()> {
        Poll::Pending
    }
}

#[test]
fn a_dropped_async_operation_does_not_fail_its_caller() {
    assert_eq!(smol::block_on(abandon(2)), 3);
}
