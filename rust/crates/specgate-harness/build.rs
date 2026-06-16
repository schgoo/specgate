use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let source_path = manifest_dir.join("src").join("traced_counter.rs");
    specgate::write_annotation_registry(
        &source_path,
        &manifest_dir,
        "specgate_harness::traced_counter",
    )
    .expect("annotation registry should be written");
    println!("cargo:rerun-if-changed={}", source_path.display());
}
