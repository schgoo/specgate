use serde::Deserialize;
use specgate::{SpecEvent, ToNativeValue, spec_operation};

specgate::spec_component!("fixture.typed_async");

#[derive(Debug, Deserialize, SpecEvent)]
pub struct Item {
    #[spec_event]
    pub value: i32,
}

#[derive(SpecEvent)]
pub struct Client {
    endpoint: String,
    _environment: Box<dyn std::any::Any>,
    calls: i32,
}

#[derive(SpecEvent)]
pub struct Offset {
    #[spec_event]
    pub value: i32,
}

impl Client {
    pub fn new(endpoint: String, environment: Box<dyn std::any::Any>) -> Self {
        Self {
            endpoint,
            _environment: environment,
            calls: 0,
        }
    }

    #[spec_operation("find_items", types = "crate::Item")]
    pub async fn find_by_type_name<T: for<'de> Deserialize<'de> + ToNativeValue + 'static>(
        &mut self,
        #[spec_input("type_name")] name: String,
        offset: &Offset,
    ) -> Vec<T> {
        assert_eq!(name, "Item");
        assert_eq!(offset.value, 3);
        // Exercise a reactor-backed future, not just an immediately ready one.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        self.calls += 1;
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let mut stream = tokio::net::TcpStream::connect(&self.endpoint)
            .await
            .expect("connect to the annotated setup's HTTP server");
        stream
            .write_all(b"GET /items HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("send HTTP request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read the setup's HTTP response");
        let response = std::str::from_utf8(&response).expect("UTF-8 HTTP response");
        let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response headers");
        assert!(headers.starts_with("HTTP/1.1 200 OK"));
        serde_json::from_str(body).expect("the annotated setup supplies valid typed JSON")
    }

    #[spec_operation("calls")]
    pub fn calls(&self) -> i32 {
        self.calls
    }
}

#[cfg(test)]
#[path = "setup.rs"]
mod tests;
