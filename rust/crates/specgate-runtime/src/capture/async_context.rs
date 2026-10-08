//! Per-future native-capture context propagation.

use super::{CollectorHandle, SESSION, current_collector, lock_collector};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Capture context carried by an annotated future from construction onward.
#[derive(Debug)]
#[doc(hidden)]
pub struct CaptureContext {
    collector: Option<CollectorHandle>,
    current_operation: Option<usize>,
}

#[doc(hidden)]
#[must_use]
pub fn capture_async_context() -> CaptureContext {
    let collector = current_collector();
    let current_operation = collector
        .as_ref()
        .and_then(|collector| lock_collector(collector).as_ref().and_then(|state| state.current_operation));
    CaptureContext {
        collector,
        current_operation,
    }
}

struct InstalledContext<'a> {
    context: &'a mut CaptureContext,
    previous_collector: Option<CollectorHandle>,
    previous_operation: Option<usize>,
}

impl<'a> InstalledContext<'a> {
    fn install(context: &'a mut CaptureContext) -> Self {
        let previous_collector = SESSION.with(|slot| slot.replace(context.collector.clone()));
        let previous_operation = swap_operation(context.collector.as_ref(), context.current_operation);
        Self {
            context,
            previous_collector,
            previous_operation,
        }
    }
}

impl Drop for InstalledContext<'_> {
    fn drop(&mut self) {
        self.context.current_operation = swap_operation(self.context.collector.as_ref(), self.previous_operation);
        SESSION.with(|slot| *slot.borrow_mut() = self.previous_collector.take());
    }
}

fn swap_operation(collector: Option<&CollectorHandle>, value: Option<usize>) -> Option<usize> {
    let collector = collector?;
    let mut guard = lock_collector(collector);
    let state = guard.as_mut()?;
    std::mem::replace(&mut state.current_operation, value)
}

#[derive(Debug)]
#[doc(hidden)]
pub struct InstrumentedFuture<F> {
    context: CaptureContext,
    future: Pin<Box<F>>,
}

/// Wrap an annotated async body in its construction-time capture context.
///
/// The wrapper preserves the inner future's auto traits: it is [`Send`] when
/// `F` is `Send`, while non-`Send` futures remain usable on local executors.
#[doc(hidden)]
pub fn instrument_async_operation<F: Future>(context: CaptureContext, future: F) -> InstrumentedFuture<F> {
    InstrumentedFuture {
        context,
        future: Box::pin(future),
    }
}

impl<F: Future> Future for InstrumentedFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { context, future } = self.get_mut();
        if context.collector.is_none() {
            return future.as_mut().poll(cx);
        }
        let installed = InstalledContext::install(context);
        let result = future.as_mut().poll(cx);
        drop(installed);
        result
    }
}
