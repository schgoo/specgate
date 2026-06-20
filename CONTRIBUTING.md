# Contributing to SpecGate

Thank you for your interest in contributing to SpecGate!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/<you>/specgate.git`
3. Set up the Rust toolchain: `cd rust && rustup show` (uses `rust-toolchain.toml`)
4. Build: `cargo build --workspace`
5. Test: `cargo test --workspace`

## Development Workflow

### Before submitting a PR

Run all checks:

```bash
cd rust
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt -- --check
cargo deny check all
```

### Spec-first development

SpecGate follows a spec-first workflow:

1. Write or update a `.spec.yaml` file
2. Validate: `cargo run -p specgate-cli -- validate <spec-dir>`
3. Implement the spec (annotate code with `#[spec_operation]`, etc.)
4. Verify: `cargo run -p specgate-cli -- run <spec-file>`

See `.github/skills/implement-spec.md` for the full implementation workflow.

### Code style

- Run `cargo fmt` before committing
- All clippy warnings must be resolved
- Follow the [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/) for resilience

### Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new feature
- `fix:` — bug fix
- `chore:` — maintenance
- `docs:` — documentation
- `refactor:` — code restructuring
- `spec:` — spec file changes
- `schema:` — schema changes

## License

By contributing, you agree that your contributions will be licensed under the
MIT OR Apache-2.0 license (see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE)).
