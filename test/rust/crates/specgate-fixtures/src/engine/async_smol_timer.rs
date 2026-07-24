// Reactor-backed async operation (smol). `smol::Timer` only makes progress when
// driven by a real executor with its reactor (`smol::block_on`). Under the old
// no-op-waker busy-loop runner it would spin forever; under the single
// top-level smol runtime entry (the default when a binding declares no
// `runtime:`) it completes. Regression fixture for real async support.
use specgate::*;
use std::time::Duration;

#[spec_operation("smol_delay", spec = "fixture.async_smol_timer")]
pub async fn smol_delay() -> String {
    smol::Timer::after(Duration::from_millis(1)).await;
    "smol done".to_string()
}
