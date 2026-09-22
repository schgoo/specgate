use specgate::spec_operation;

#[spec_operation("fetch", spec = "fixture.async_fetch")]
pub async fn fetch(url: &str) -> String {
    std::future::ready(format!("response from {url}")).await
}

#[test]
fn async_operation_is_discovery_valid_without_native_capture() {
    let operation = specgate::__rt::SPECGATE_OPS
        .iter()
        .find(|operation| operation.component == "fixture.async_fetch" && operation.name == "fetch")
        .expect("async fetch metadata");
    assert!(operation.is_async);
    assert_eq!(operation.params, &[("url", "& str")]);
}
