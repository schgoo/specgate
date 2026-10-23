use super::*;
use specgate::spec_setup;
use std::io::{Read as _, Write as _};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct HttpEnvironment {
    endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl HttpEnvironment {
    fn new(body: String) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !signal.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                        let mut request = [0_u8; 1024];
                        let count = stream.read(&mut request).unwrap();
                        assert!(count > 0);
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(1)),
                    Err(error) => panic!("test HTTP accept failed: {error}"),
                }
            }
        });
        Self {
            endpoint,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for HttpEnvironment {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().expect("test HTTP server completed");
    }
}

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
