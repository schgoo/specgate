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
