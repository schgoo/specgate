// Async operation — tests that harness generates async runner.
use specgate_annotations::*;

#[spec_operation("fetch")]
pub async fn fetch(url: &str) -> String {
    format!("response from {url}")
}
