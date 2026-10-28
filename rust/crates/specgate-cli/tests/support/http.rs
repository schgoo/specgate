use std::io::{BufRead as _, Write as _};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct HttpEnvironment {
    pub(super) endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl HttpEnvironment {
    pub(super) fn new(body: String) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !signal.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => serve_request(stream, &body),
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

fn serve_request(stream: std::net::TcpStream, body: &str) {
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut stream = std::io::BufReader::new(stream);
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        let count = stream.read_until(b'\n', &mut request).expect("read test HTTP request headers");
        assert_ne!(count, 0, "client disconnected before the HTTP request headers were complete");
    }
    write!(
        stream.get_mut(),
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write test HTTP response");
}

impl Drop for HttpEnvironment {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Err(payload) = self.worker.take().unwrap().join() {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            if std::thread::panicking() {
                eprintln!("test HTTP server failed during cleanup: {message}");
            } else {
                panic!("test HTTP server failed: {message}");
            }
        }
    }
}
