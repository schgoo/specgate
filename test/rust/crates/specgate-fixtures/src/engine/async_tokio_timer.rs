// Reactor-backed async operation (tokio). `tokio::time::sleep` requires a tokio
// runtime context; without one it panics ("no reactor running"). It completes
// only when the runner establishes a tokio runtime — selected via the binding
// target's `runtime: tokio`. Regression fixture for real async support.
use specgate::*;
use std::time::Duration;

#[spec_operation("tokio_delay", spec = "fixture.async_tokio_timer")]
pub async fn tokio_delay() -> String {
    tokio::time::sleep(Duration::from_millis(1)).await;
    "tokio done".to_string()
}
