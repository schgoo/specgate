//! Focused support adapter and Cargo identity tests.

use super::*;

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
#[cfg(windows)]
use std::net::TcpStream;
use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

const RUNTIME_VERSION: &str = "99.0.0";

mod cache;
mod source;

#[test]
fn executor_dependencies_keep_aliases_and_defer_unsupported_sources() {
    let metadata: CargoMetadata = serde_json::from_value(serde_json::json!({
        "packages": [
            {"id": "arty", "name": "arty", "version": "0.5.0", "manifest_path": "arty/Cargo.toml",
             "source": "git+https://example.invalid/arty"},
            {"id": "tokio", "name": "tokio", "version": "1.48.0", "manifest_path": "tokio/Cargo.toml",
             "source": "registry+https://github.com/rust-lang/crates.io-index"},
            {"id": "smol", "name": "smol", "version": "2.0.2", "manifest_path": "smol/Cargo.toml",
             "source": "registry+https://github.com/rust-lang/crates.io-index"}
        ],
        "resolve": {"nodes": [
            {"id": "candidate", "deps": [
                {"pkg": "arty", "name": "candidate_engine", "dep_kinds": [{"kind": "dev"}]},
                {"pkg": "tokio", "name": "build_engine", "dep_kinds": [{"kind": "build"}]}
            ]},
            {"id": "arty", "deps": [
                {"pkg": "smol", "name": "smol", "dep_kinds": [{"kind": null}]}
            ]}
        ]}
    }))
    .unwrap();
    let executors = executor_dependencies(&metadata, "candidate").unwrap();
    assert_eq!(executors.len(), 1);
    let dependency = &executors["candidate_engine"];
    assert_eq!(dependency.package.as_str(), "arty");
    assert!(dependency.source.as_ref().unwrap_err().contains("unsupported arty Cargo source"));
}

fn run_capture(candidate: impl AsRef<Path>, runner: impl AsRef<Path>, context: &CandidatePackage) {
    let candidate = candidate.as_ref();
    let runner = runner.as_ref();
    std::fs::create_dir_all(runner.join("src")).unwrap();
    let cargo = runner_cargo(
        "registry-identity-runner",
        BTreeMap::from([
            (
                "candidate".to_string(),
                Dependency::local(context.package.clone(), context.version.clone(), context.path.clone()).unwrap(),
            ),
            ("specgate_runtime".to_string(), Dependency::from_source(&context.runtime).unwrap()),
        ]),
    )
    .unwrap();
    std::fs::write(runner.join("Cargo.toml"), cargo.manifest).unwrap();
    let config = cargo.config.map(|config| {
        let path = runner.join("registry-config.toml");
        std::fs::write(&path, config).unwrap();
        path
    });
    std::fs::write(
        runner.join("src").join("main.rs"),
        "fn main() {\n    candidate::start_capture();\n    assert!(specgate_runtime::capture_active());\n}\n",
    )
    .unwrap();

    let mut command = Command::new(cargo_bin());
    command.arg("run").arg("--quiet");
    if let Some(config) = config {
        command.arg("--config").arg(config);
    }
    let output = command
        .arg("--manifest-path")
        .arg(runner.join("Cargo.toml"))
        .current_dir(candidate)
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", runner.join("target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "runner and candidate used distinct runtime capture state:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TestProject {
    cache: InvocationCache,
}

impl TestProject {
    fn create(label: impl AsRef<str>) -> Self {
        Self {
            cache: InvocationCache::create(CacheScope::new("integration-tests"), CacheLabel::new(label)).unwrap(),
        }
    }
    fn path(&self) -> &Path {
        self.cache.path()
    }
}

fn package_archive(runtime: impl AsRef<Path>) -> (String, Vec<u8>) {
    let runtime = runtime.as_ref();
    std::fs::create_dir_all(runtime.join("src")).unwrap();
    std::fs::write(
            runtime.join("Cargo.toml"),
            format!(
                "[package]\nname=\"specgate-runtime\"\nversion=\"{RUNTIME_VERSION}\"\nedition=\"2024\"\ndescription=\"registry identity fixture\"\nlicense=\"MIT\"\n[workspace]\n"
            ),
        )
        .unwrap();
    std::fs::write(
            runtime.join("src").join("lib.rs"),
            "use std::sync::atomic::{AtomicBool, Ordering};\nstatic CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);\npub fn start_capture() { CAPTURE_ACTIVE.store(true, Ordering::SeqCst); }\npub fn capture_active() -> bool { CAPTURE_ACTIVE.load(Ordering::SeqCst) }\n",
        )
        .unwrap();
    let package_target = runtime.join("target");
    let output = Command::new(cargo_bin())
        .arg("package")
        .arg("--allow-dirty")
        .arg("--no-verify")
        .arg("--manifest-path")
        .arg(runtime.join("Cargo.toml"))
        .current_dir(runtime)
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", &package_target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to package registry runtime:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let archive_name = format!("specgate-runtime-{RUNTIME_VERSION}.crate");
    let archive = package_target.join("package").join(&archive_name);
    let archive = std::fs::read(&archive).unwrap();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let index_entry = serde_json::json!({
        "name": "specgate-runtime",
        "vers": RUNTIME_VERSION,
        "deps": [],
        "cksum": checksum,
        "features": {},
        "yanked": false
    })
    .to_string();
    (index_entry, archive)
}

fn create_registry(registry: impl AsRef<Path>, index_entry: impl AsRef<str>, archive: impl AsRef<[u8]>) {
    let registry = registry.as_ref();
    let index_entry = index_entry.as_ref();
    let archive = archive.as_ref();
    let index = registry.join("index");
    std::fs::create_dir_all(index.join("sp").join("ec")).unwrap();
    let archive_name = format!("specgate-runtime-{RUNTIME_VERSION}.crate");
    std::fs::write(registry.join(archive_name), archive).unwrap();
    std::fs::write(index.join("sp").join("ec").join("specgate-runtime"), format!("{index_entry}\n")).unwrap();
}

fn registry_candidate(candidate: impl AsRef<Path>, registry: impl AsRef<Path>) {
    let candidate = candidate.as_ref();
    let registry = registry.as_ref();
    std::fs::create_dir_all(candidate.join("src")).unwrap();
    std::fs::create_dir_all(candidate.join(".cargo")).unwrap();
    std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime=\"={RUNTIME_VERSION}\"\n[workspace]\n"
            ),
        )
        .unwrap();
    std::fs::write(
        candidate.join("src").join("lib.rs"),
        "pub fn start_capture() { specgate_runtime::start_capture(); }\n",
    )
    .unwrap();
    let registry = toml::Value::String(cargo_path(registry).unwrap()).to_string();
    std::fs::write(
        candidate.join(".cargo").join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with=\"specgate-test-registry\"\n[source.specgate-test-registry]\nlocal-registry={registry}\n"
        ),
    )
    .unwrap();
}

fn alternate_candidate(candidate: impl AsRef<Path>, registry_index: impl AsRef<str>) {
    let candidate = candidate.as_ref();
    let registry_index = registry_index.as_ref();
    std::fs::create_dir_all(candidate.join("src")).unwrap();
    std::fs::create_dir_all(candidate.join(".cargo")).unwrap();
    std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime={{version=\"={RUNTIME_VERSION}\",registry=\"candidate-registry\"}}\n[workspace]\n"
            ),
        )
        .unwrap();
    std::fs::write(
        candidate.join("src").join("lib.rs"),
        "pub fn start_capture() { specgate_runtime::start_capture(); }\n",
    )
    .unwrap();
    std::fs::write(
        candidate.join(".cargo").join("config.toml"),
        format!(
            "[registries.candidate-registry]\nindex={}\n",
            toml::Value::String(registry_index.to_string())
        ),
    )
    .unwrap();
}

fn path_runtime(runtime: impl AsRef<Path>, version: impl AsRef<str>) {
    let runtime = runtime.as_ref();
    let version = version.as_ref();
    std::fs::create_dir_all(runtime.join("src")).unwrap();
    std::fs::write(
        runtime.join("Cargo.toml"),
        format!("[package]\nname=\"specgate-runtime\"\nversion=\"{version}\"\nedition=\"2024\"\n[workspace]\n"),
    )
    .unwrap();
    std::fs::write(
            runtime.join("src").join("lib.rs"),
            "use std::sync::atomic::{AtomicBool, Ordering};\nstatic CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);\npub fn start_capture() { CAPTURE_ACTIVE.store(true, Ordering::SeqCst); }\npub fn capture_active() -> bool { CAPTURE_ACTIVE.load(Ordering::SeqCst) }\n",
        )
        .unwrap();
}

fn path_candidate(candidate: impl AsRef<Path>, runtime: impl AsRef<Path>) {
    let candidate = candidate.as_ref();
    let runtime = runtime.as_ref();
    std::fs::create_dir_all(candidate.join("src")).unwrap();
    std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime={{path={}}}\n[workspace]\n",
                toml::Value::String(cargo_path(runtime).unwrap())
            ),
        )
        .unwrap();
    std::fs::write(
        candidate.join("src").join("lib.rs"),
        "pub fn start_capture() { specgate_runtime::start_capture(); }\n",
    )
    .unwrap();
}

struct SparseRegistry {
    index: String,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SparseRegistry {
    fn start(index_entry: String, archive: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        // Cargo caches package contents across runs, even when a listener port is recycled.
        let prefix = format!("/{:x}/", Sha256::digest(&archive));
        let index = format!("sparse+http://{address}{prefix}");
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let thread = std::thread::spawn(move || {
            while !server_shutdown.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _address)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                        serve_request(stream, address, &prefix, &index_entry, &archive);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("sparse registry server failed: {error}"),
                }
            }
        });
        Self {
            index,
            shutdown,
            thread: Some(thread),
        }
    }

    fn index(&self) -> &str {
        &self.index
    }
}

impl Drop for SparseRegistry {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}

#[test]
fn registry_content_identity() {
    for archive in [b"original runtime".as_slice(), b"revised runtime".as_slice()] {
        let registry = SparseRegistry::start("{}".to_string(), archive.to_vec());
        let prefix = format!("/{:x}/", Sha256::digest(archive));
        assert!(
            registry.index().ends_with(&prefix),
            "recycled registry ports must not reuse cached packages with different contents: {}",
            registry.index()
        );
    }
}

#[cfg(windows)]
#[test]
fn request_wait() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(address).unwrap();
    let (stream, _) = listener.accept().unwrap();
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let server = std::thread::spawn(move || serve_request(stream, address, "/fixture/", "{}", []));

    std::thread::sleep(Duration::from_millis(50));
    client
        .write_all(b"GET /fixture/config.json HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap();

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with(&format!(r#"{{"dl":"http://{address}/fixture/api/v1/crates"}}"#)));
}

fn serve_request(
    mut stream: impl Read + Write,
    address: SocketAddr,
    prefix: &str,
    index_entry: impl AsRef<str>,
    archive: impl AsRef<[u8]>,
) {
    let index_entry = index_entry.as_ref();
    let archive = archive.as_ref();
    let mut request = [0_u8; 8192];
    let length = stream.read(&mut request).unwrap();
    let request = String::from_utf8_lossy(&request[..length]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|path| path.strip_prefix(prefix));
    let config = format!(r#"{{"dl":"http://{address}{prefix}api/v1/crates"}}"#);
    let (status, content_type, body) = match path {
        Some("config.json") => ("200 OK", "application/json", config.as_bytes()),
        Some("sp/ec/specgate-runtime") => ("200 OK", "text/plain", index_entry.as_bytes()),
        Some(path) if path == format!("api/v1/crates/specgate-runtime/{RUNTIME_VERSION}/download") => {
            ("200 OK", "application/octet-stream", archive)
        }
        _ => ("404 Not Found", "text/plain", &[] as &[u8]),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
}

fn fake_system(
    filesystem: system::FakeFs,
    results: Vec<Result<system::ProcessOutput, String>>,
    environment: BTreeMap<String, OsString>,
) -> system::System {
    system::System::builder((filesystem, system::FakeProcess::queued(results)))
        .environment(environment)
        .process_id(4242)
        .build()
}

fn metadata_json() -> Vec<u8> {
    br#"{"packages":[{"id":"candidate 0.1.0 (path+file:///candidate)","name":"candidate","version":"0.1.0","manifest_path":"candidate/Cargo.toml","source":null,"targets":[{"name":"candidate","kind":["lib"]}]},{"id":"specgate-runtime 0.6.0 (registry+https://github.com/rust-lang/crates.io-index)","name":"specgate-runtime","version":"0.6.0","manifest_path":"cargo/specgate-runtime/Cargo.toml","source":"registry+https://github.com/rust-lang/crates.io-index"}],"resolve":{"nodes":[{"id":"candidate 0.1.0 (path+file:///candidate)","deps":[{"pkg":"specgate-runtime 0.6.0 (registry+https://github.com/rust-lang/crates.io-index)"}]},{"id":"specgate-runtime 0.6.0 (registry+https://github.com/rust-lang/crates.io-index)","deps":[]}]}}"#.to_vec()
}
