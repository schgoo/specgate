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

# Run the conformance self-hosting test: the harness validates the conformance
# ledger by running run_spec on specs/specgate.conformance.spec.yaml, exercising
# the multi-target byte-identity path. Doubly-nested and #[ignore]d like the
# harness one.
conformance-self-host:
    cd rust && cargo test -p specgate --test conformance_self_host -- --ignored

# Run the CLI self-hosting test: the harness validates the CLI's own spec by
# running run_spec on specs/specgate.cli.spec.yaml, exercising the CLI's
# validate/run operations. Doubly-nested and #[ignore]d like the harness one.
cli-self-host:
    cd rust && cargo test -p specgate-cli --test cli_self_host -- --ignored

# Run the code-coverage test: runs a spec through the harness with
# instrumentation and checks the implementation crate's coverage is measured.
# Needs the llvm-tools component (`rustup component add llvm-tools-preview`);
# tolerates its absence (degrades to "unavailable"). #[ignore]d like the others.
coverage:
    cd rust && cargo test -p specgate-harness --test coverage -- --ignored

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

# Verify `specgate extract` still reproduces the committed golden specs for the
# extract fixture crates, byte-for-byte. See scripts/extract-goldens.ps1 for the
# fixture list. Regenerate the goldens with `just extract-update` after an
# intentional emitter change.
extract-check:
    pwsh -NoLogo -NoProfile -File scripts/extract-goldens.ps1 -Mode check

# Regenerate the committed extract goldens (spec + binding) from the fixture
# crates. Run after an intentional change to the extractor's deterministic
# output; review the resulting diff before committing.
extract-update:
    pwsh -NoLogo -NoProfile -File scripts/extract-goldens.ps1 -Mode update

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

# Build C# fixtures
dotnet-build:
    dotnet build

# Run C# tests
dotnet-test:
    dotnet test

# Apply C# formatting + analyzer fixes (analog of `just format` / clippy --fix)
format-cs:
    dotnet format SpecGate.slnx

# Check C# formatting, style, and analyzers (analog of clippy -D warnings +
# fmt --check). Build already enforces analyzers as errors; this also gates
# whitespace/style. Fails if any file would change.
format-check-cs:
    dotnet format SpecGate.slnx --verify-no-changes

# Run all pre-PR checks
check: build test clippy format-check deny validate extract-check readme-check self-host conformance-self-host cli-self-host coverage dotnet-build dotnet-test format-check-cs
