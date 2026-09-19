set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]

_default:
    @just --list

build:
    cd rust && cargo build --workspace --all-targets
    cd test/rust && cargo build --workspace --all-targets

test:
    cd rust && cargo test --workspace
    cd test/rust && cargo test --workspace

clippy:
    cd rust && cargo clippy --workspace --all-targets -- -D warnings
    cd test/rust && cargo clippy --workspace --all-targets -- -D warnings

format-check:
    cd rust && cargo fmt -- --check
    cd test/rust && cargo fmt -- --check

format:
    cd rust && cargo fmt
    cd test/rust && cargo fmt

deny:
    cd rust && cargo deny check licenses

readme:
    cd rust && cargo doc2readme -p specgate-runtime --lib --template crates/README.j2 --out crates/specgate-runtime/README.md
    cd rust && cargo doc2readme -p specgate-annotations-macros --lib --template crates/README.j2 --out crates/specgate-annotations-macros/README.md
    cd rust && cargo doc2readme -p specgate-discovery --lib --template crates/README.j2 --out crates/specgate-discovery/README.md
    cd rust && cargo doc2readme -p specgate --lib --template crates/README.j2 --out crates/specgate/README.md
    cd rust && cargo doc2readme -p specgate-ctsc --lib --template crates/README.j2 --out crates/specgate-ctsc/README.md
    cd rust && cargo doc2readme -p specgate-cli --lib --template crates/README.j2 --out crates/specgate-cli/README.md

readme-check:
    cd rust && cargo doc2readme -p specgate-runtime --lib --template crates/README.j2 --out crates/specgate-runtime/README.md --check
    cd rust && cargo doc2readme -p specgate-annotations-macros --lib --template crates/README.j2 --out crates/specgate-annotations-macros/README.md --check
    cd rust && cargo doc2readme -p specgate-discovery --lib --template crates/README.j2 --out crates/specgate-discovery/README.md --check
    cd rust && cargo doc2readme -p specgate --lib --template crates/README.j2 --out crates/specgate/README.md --check
    cd rust && cargo doc2readme -p specgate-ctsc --lib --template crates/README.j2 --out crates/specgate-ctsc/README.md --check
    cd rust && cargo doc2readme -p specgate-cli --lib --template crates/README.j2 --out crates/specgate-cli/README.md --check

ctsc-validate:
    pwsh -NoLogo -NoProfile -File scripts/ctsc-validate.ps1

ctsc-smoke:
    pwsh -NoLogo -NoProfile -File scripts/ctsc-smoke.ps1

package-smoke:
    pwsh -NoLogo -NoProfile -File scripts/package-smoke.ps1

dotnet-build:
    dotnet build SpecGate.slnx

dotnet-test:
    dotnet test SpecGate.slnx --no-build

format-cs:
    dotnet format SpecGate.slnx

format-check-cs:
    dotnet format SpecGate.slnx --verify-no-changes

check: build test clippy format-check deny readme-check ctsc-validate ctsc-smoke dotnet-build dotnet-test format-check-cs
