set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]
set dotenv-path := "./constants.env"

_default:
    @just --list

# Build the workspace
build:
    cd rust && cargo build --workspace --all-targets

# Run all tests
test:
    cd rust && cargo test --workspace

# Run the self-hosting test: the harness validates its own spec by running
# run_spec on specs/specgate.harness.spec.yaml. Doubly-nested (shells out to
# cargo per case), so it is #[ignore]d in the normal suite and run explicitly.
self-host:
    cd rust && cargo test -p specgate --test harness_self_host -- --ignored

# Run clippy
clippy:
    cd rust && cargo clippy --workspace --all-targets -- -D warnings

# Check formatting
format-check:
    cd rust && cargo fmt -- --check

# Apply formatting
format:
    cd rust && cargo fmt

# Run cargo deny (licenses only — advisory DB has CVSS 4.0 compat issues)
deny:
    cd rust && cargo deny check licenses

# Run specgate validate on all specs
validate:
    cd rust && cargo run -p specgate-cli --quiet -- validate ../specs

# Generate README.md for each crate from lib.rs doc comments
readme:
    cd rust && cargo doc2readme -p specgate-runtime --lib --template crates/README.j2 --out crates/specgate-runtime/README.md
    cd rust && cargo doc2readme -p specgate-annotations-macros --lib --template crates/README.j2 --out crates/specgate-annotations-macros/README.md
    cd rust && cargo doc2readme -p specgate-annotations --lib --template crates/README.j2 --out crates/specgate-annotations/README.md
    cd rust && cargo doc2readme -p specgate-types --lib --template crates/README.j2 --out crates/specgate-types/README.md
    cd rust && cargo doc2readme -p specgate-harness --lib --template crates/README.j2 --out crates/specgate-harness/README.md
    cd rust && cargo doc2readme -p specgate --lib --template crates/README.j2 --out crates/specgate/README.md
    cd rust && cargo doc2readme -p specgate-cli --lib --template crates/README.j2 --out crates/specgate-cli/README.md

# Check READMEs are up to date
readme-check:
    cd rust && cargo doc2readme -p specgate-runtime --lib --template crates/README.j2 --out crates/specgate-runtime/README.md --check
    cd rust && cargo doc2readme -p specgate-annotations-macros --lib --template crates/README.j2 --out crates/specgate-annotations-macros/README.md --check
    cd rust && cargo doc2readme -p specgate-annotations --lib --template crates/README.j2 --out crates/specgate-annotations/README.md --check
    cd rust && cargo doc2readme -p specgate-types --lib --template crates/README.j2 --out crates/specgate-types/README.md --check
    cd rust && cargo doc2readme -p specgate-harness --lib --template crates/README.j2 --out crates/specgate-harness/README.md --check
    cd rust && cargo doc2readme -p specgate --lib --template crates/README.j2 --out crates/specgate/README.md --check
    cd rust && cargo doc2readme -p specgate-cli --lib --template crates/README.j2 --out crates/specgate-cli/README.md --check

# Run all pre-PR checks
check: build test clippy format-check deny validate readme-check self-host
