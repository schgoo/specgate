// Async operation — tests that harness generates async runner. The body awaits
// an immediately-ready future so the `await` machinery is genuinely exercised
// without needing a reactor; the C# conformance mirror awaits `Task.FromResult`.
use specgate::*;

#[spec_operation("fetch", spec = "fixture.async_fetch")]
pub async fn fetch(url: &str) -> String {
    std::future::ready(format!("response from {url}")).await
}
