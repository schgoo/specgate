#[spec_operation("tokio_delay", spec = "fixture.async_tokio_timer")]
pub async fn tokio_delay() -> String {
    tokio::time::sleep(Duration::from_millis(1)).await;
    "tokio done".to_string()
}
