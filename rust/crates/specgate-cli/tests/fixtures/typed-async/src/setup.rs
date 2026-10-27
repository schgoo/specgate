use super::*;
use specgate::spec_setup;
#[path = "../../../support/http.rs"]
mod http;
use http::HttpEnvironment;

#[spec_setup("find_items", spec = "fixture.typed_async")]
#[spec_setup("calls", spec = "fixture.typed_async")]
fn make_client(#[spec_input("response")] body: String) -> Client {
    let environment = HttpEnvironment::new(body);
    Client::new(environment.endpoint.clone(), Box::new(environment))
}

#[spec_setup("find_items", fills = "offset", spec = "fixture.typed_async")]
fn make_offset(#[spec_input("initial")] seed: i32) -> Offset {
    Offset { value: seed }
}

#[test]
fn typed_async_client() {
    assert!(
        std::env::var_os("SPECGATE_TEST_CAPTURE").is_none(),
        "replay must not rerun the ordinary test body"
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut client = make_client(r#"[{"value":7},{"value":11}]"#.to_string());
            let offset = make_offset(3);
            assert_eq!(client.find_by_type_name::<Item>("Item".to_string(), &offset).await[0].value, 7);
            assert_eq!(client.find_by_type_name::<Item>("Item".to_string(), &offset).await.len(), 2);
            assert_eq!(client.calls(), 2);
        });
}
