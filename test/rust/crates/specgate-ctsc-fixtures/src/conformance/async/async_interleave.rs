//! Two annotated async operations interleaved inside one enclosing operation.
//!
//! Interleaved operations do not nest: the first to complete is not the most
//! recently opened. Each instrumented future carries the context it was
//! constructed with, so both record complete spans parented to `interleave`
//! rather than to each other, and neither poisons the run.
//!
//! The captured trace therefore carries two *overlapping* sibling operation
//! spans, which `ctsc.strict/0.1.0` comparison rejects outside
//! `conformance.parallel`. The matrix row is `replay.mode: "unsupported"`, so
//! comparison never runs against it.
//!
//! No `spec_trace!` observation is emitted here. Discovery has no link-time
//! observation metadata yet, so a captured observation fails linked validation
//! (see `component/fixture.fallible_unit`'s `observation-not-declared`).

use specgate::spec_operation;
use std::pin::Pin;
use std::task::{Context, Poll};

#[spec_operation("double", spec = "fixture.async_interleave")]
pub async fn double(value: i32) -> i32 {
    YieldOnce::default().await;
    value * 2
}

/// Drives two `double` futures round-robin by hand.
///
/// The schedule is written out rather than handed to an executor, so operation
/// open order — and therefore the synthesized span IDs and the logical clock —
/// is deterministic. A worker pool would make the golden flaky.
#[spec_operation("interleave", spec = "fixture.async_interleave")]
pub async fn interleave(value: i32) -> i32 {
    let mut context = Context::from_waker(std::task::Waker::noop());
    let mut first = Box::pin(double(value));
    let mut second = Box::pin(double(value + 1));

    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(second.as_mut().poll(&mut context).is_pending());
    let Poll::Ready(first) = first.as_mut().poll(&mut context) else {
        panic!("the first operation completes on its second poll");
    };
    let Poll::Ready(second) = second.as_mut().poll(&mut context) else {
        panic!("the second operation completes on its second poll");
    };
    first + second
}

/// Yields exactly once, so two of these can be interleaved by hand.
#[derive(Default)]
struct YieldOnce {
    polled: bool,
}

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.polled {
            Poll::Ready(())
        } else {
            self.polled = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[test]
fn two_interleaved_async_operations_both_record_complete_spans() {
    assert_eq!(smol::block_on(interleave(2)), 10);
}
