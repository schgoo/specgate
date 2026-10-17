# Specgate-Discovery

[![crates.io](https://img.shields.io/crates/v/specgate-discovery.svg)](https://crates.io/crates/specgate-discovery)
[![docs.rs](https://docs.rs/specgate-discovery/badge.svg)](https://docs.rs/specgate-discovery)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Native implementation discovery for `SpecGate`’s CTSC workflow.

This crate owns strict implementation binding, Rust link-time metadata discovery,
C# compiled-assembly reflection, raw invocation metadata, and deterministic
semantic schema normalization. The four crate-root `discover_*` functions
are the canonical primary workflow. Supporting APIs and result models use
cohesive module paths:

* [`binding`][__link0] parses and resolves implementation bindings;
* [`identity`][__link1] defines strongly typed semantic identities;
* [`output`][__link2] exposes workflow result models;
* [`registry`][__link3] parses and queries raw discovery registries;
* [`schema`][__link4] normalizes semantic schemas;
* [`types`][__link5] and [`setup`][__link6] expose schema type and setup helpers;
* [`runner`][__link7] provides generated-runner Cargo and execution support;
* the `test-util` feature exposes [`test_util`][__link8] failure-injection harnesses.

A caller can resolve a component implementation with
[`binding::resolve_implementation`][__link9] and pass it to [`discover_resolved`][__link10].
Discovery verifies that the selected sources actually declare the component.
The example compiles without invoking Cargo or requiring a fixture at
documentation-test time.

## Examples

```rust
use specgate_discovery::{binding, discover_resolved};

let implementation = binding::resolve_implementation("specgate.yaml", "com.example.component", None)?;
let discovered = discover_resolved(implementation)?;
println!("{}", discovered.schema.component);
```


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

 [__cargo_doc2readme_dependencies_info]: ggGmYW0CYXZlMC43LjNhdIQbJSusbBjLO7EbSlASCvKRTqwbmd2gsLYkxMobU3WiDiuhvKthYvRhcoQbFFoQlnIOSW4bwqJHaCjLK94brNYM-_Vdj0wbDZAOkO__de5hZIGDcnNwZWNnYXRlLWRpc2NvdmVyeWUwLjYuMHJzcGVjZ2F0ZV9kaXNjb3Zlcnk
 [__link0]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/binding/index.html
 [__link1]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/identity/index.html
 [__link10]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/?search=discover_resolved
 [__link2]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/output/index.html
 [__link3]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/registry/index.html
 [__link4]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/schema/index.html
 [__link5]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/types/index.html
 [__link6]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/setup/index.html
 [__link7]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/runner/index.html
 [__link8]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/test_util/index.html
 [__link9]: https://docs.rs/specgate-discovery/0.6.0/specgate_discovery/?search=binding::resolve_implementation
