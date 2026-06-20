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

# Run all pre-PR checks
check: build test clippy format-check deny validate
