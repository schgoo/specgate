set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]

_default:
    @just --list

# C# discovery runs the prebuilt SpecGate.Discovery tool. Outside an installed
# layout nothing sits beside the specgate executable, so every recipe that
# drives C# discovery builds the tool here and points SPECGATE_DISCOVERY_PATH
# at it.
export SPECGATE_DISCOVERY_PATH := justfile_directory() / "artifacts" / "discovery"

# C# capture runs the prebuilt weaver, the runtime it binds to, and the MSBuild
# targets that inject them into the build. They sit in one directory, beside the
# specgate executable when installed, otherwise here via SPECGATE_WEAVER_PATH.
# The runtime is built for net8.0, the lowest framework a woven assembly can run on.
export SPECGATE_WEAVER_PATH := justfile_directory() / "artifacts" / "weaver"

# rustfmt follows #[path] modules, but not include! item fragments that retain
# the original shared module identities. Keep those four real sources covered.
fixture_fragments := "test/behavior/cases/comp.core/rust/component.rs test/behavior/cases/comp.app/rust/component.rs test/behavior/cases/fixture.async_smol_timer/rust/async_metadata.rs test/behavior/cases/fixture.async_tokio_timer/rust/async_metadata.rs"

discovery-tool:
    dotnet build csharp/SpecGate.Discovery/SpecGate.Discovery.csproj --output "{{ SPECGATE_DISCOVERY_PATH }}"

weaver-tool:
    dotnet build csharp/SpecGate.Weaver/SpecGate.Weaver.csproj --output "{{ SPECGATE_WEAVER_PATH }}"
    dotnet build csharp/SpecGate.Runtime/SpecGate.Runtime.csproj --framework net8.0 --output "{{ SPECGATE_WEAVER_PATH }}"

build:
    cd rust && cargo build --workspace --all-targets
    cd test && cargo build --workspace --all-targets

rust-test-regressions:
    pwsh -NoLogo -NoProfile -File scripts/rust-test-tests.ps1
    pwsh -NoLogo -NoProfile -File scripts/rust-test-cargo-tests.ps1

rust-test: discovery-tool weaver-tool rust-test-regressions
    pwsh -NoLogo -NoProfile -File scripts/rust-test.ps1

test: rust-test
    cd test && cargo test --workspace

clippy:
    cd rust && cargo clippy --workspace --all-targets -- -D warnings
    cd test && cargo clippy --workspace --all-targets -- -D warnings

format-check:
    cd rust && cargo fmt -- --check
    cd test && cargo fmt -- --check
    rustfmt --edition 2024 --check {{fixture_fragments}}

format:
    cd rust && cargo fmt
    cd test && cargo fmt
    rustfmt --edition 2024 {{fixture_fragments}}

deny:
    cd rust && cargo deny check licenses

# cargo-evaluate is an internal, authenticated local review tool and is not
# installed on GitHub-hosted runners. Rule exceptions belong in
# rust/evaluate.toml with a justification, never silenced ad hoc.
# Run deterministic and semantic lint packs over the product workspace.
evaluate:
    cd rust && cargo evaluate

# Run the complete evaluator quietly, reusing every valid file/rule cache entry.
evaluate-cached:
    cd rust && cargo evaluate --quiet

# Run the fast, reproducible subset without invoking the semantic provider.
evaluate-offline:
    cd rust && cargo evaluate --quiet -Z offline

readme:
    cd rust && cargo doc2readme -p specgate-runtime --lib --template crates/README.j2 --out crates/specgate-runtime/README.md
    cd rust && cargo doc2readme -p specgate-annotations-macros --lib --template crates/README.j2 --out crates/specgate-annotations-macros/README.md
    cd rust && cargo doc2readme -p specgate-discovery --lib --template crates/README.j2 --out crates/specgate-discovery/README.md
    cd rust && cargo doc2readme -p specgate --lib --template crates/README.j2 --out crates/specgate/README.md
    cd rust && cargo doc2readme -p specgate-ctsc --lib --template crates/README.j2 --out crates/specgate-ctsc/README.md
    cd rust && cargo doc2readme -p specgate-cli --lib --template crates/README.j2 --out crates/specgate-cli/README.md

# A bullet is a commit subject, so entries stay short by construction. To write
# a release section instead of previewing:
#   pwsh scripts/changelog.ps1 -Version 0.6.0 -Apply
# Preview CHANGELOG entries from commit subjects since the last vX.Y.Z tag
changelog:
    pwsh -NoProfile -File scripts/changelog.ps1

readme-check:
    cd rust && cargo doc2readme -p specgate-runtime --lib --template crates/README.j2 --out crates/specgate-runtime/README.md --check
    cd rust && cargo doc2readme -p specgate-annotations-macros --lib --template crates/README.j2 --out crates/specgate-annotations-macros/README.md --check
    cd rust && cargo doc2readme -p specgate-discovery --lib --template crates/README.j2 --out crates/specgate-discovery/README.md --check
    cd rust && cargo doc2readme -p specgate --lib --template crates/README.j2 --out crates/specgate/README.md --check
    cd rust && cargo doc2readme -p specgate-ctsc --lib --template crates/README.j2 --out crates/specgate-ctsc/README.md --check
    cd rust && cargo doc2readme -p specgate-cli --lib --template crates/README.j2 --out crates/specgate-cli/README.md --check

ctsc-validate:
    pwsh -NoLogo -NoProfile -File scripts/ctsc-validate.ps1

# Keep public docs in sync with the code: relative links and anchors, quoted
# capture diagnostics, the CLI usage block in docs/reference/cli.md, and
# binding examples against binding-schema.json and the binding loader.
docs-check:
    pwsh -NoLogo -NoProfile -File scripts/docs-links-tests.ps1
    pwsh -NoLogo -NoProfile -File scripts/docs-links.ps1
    pwsh -NoLogo -NoProfile -File scripts/docs-diagnostics-tests.ps1
    pwsh -NoLogo -NoProfile -File scripts/docs-diagnostics.ps1
    cd rust && cargo test -p specgate-cli --bin specgate usage
    cd rust && cargo test -p specgate-discovery --test binding_examples

ctsc-smoke-regressions:
    pwsh -NoLogo -NoProfile -File scripts/ctsc-smoke-tests.ps1

ctsc-smoke: ctsc-smoke-regressions
    pwsh -NoLogo -NoProfile -File scripts/ctsc-smoke.ps1

ctsc-goldens-harness-tests:
    cd rust && cargo test -p specgate-cli --lib goldens::tests::cases

ctsc-goldens-regressions: discovery-tool weaver-tool ctsc-goldens-harness-tests
    pwsh -NoLogo -NoProfile -File scripts/ctsc-goldens-tests.ps1

ctsc-goldens-update: ctsc-goldens-regressions
    pwsh -NoLogo -NoProfile -File scripts/ctsc-goldens.ps1 -Mode Update

ctsc-goldens-check: ctsc-goldens-regressions
    pwsh -NoLogo -NoProfile -File scripts/ctsc-goldens.ps1 -Mode Check

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

check: build test clippy format-check deny readme-check docs-check ctsc-validate ctsc-smoke ctsc-goldens-check dotnet-build dotnet-test format-check-cs
