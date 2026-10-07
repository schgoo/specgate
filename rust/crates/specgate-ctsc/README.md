# Specgate-Ctsc

[![crates.io](https://img.shields.io/crates/v/specgate-ctsc.svg)](https://crates.io/crates/specgate-ctsc)
[![docs.rs](https://docs.rs/specgate-ctsc/badge.svg)](https://docs.rs/specgate-ctsc)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

CTSC capture, registry, replay, validation, and strict comparison.

The semantic modules are the only public paths: [`capture`][__link0] converts
crate-owned evidence to deterministic OTLP JSON, [`registry`][__link1] encodes
normalized schemas, [`replay`][__link2] verifies bundle bytes, and [`validation`][__link3]
plus [`comparison`][__link4] inspect artifacts without I/O when byte APIs are used.

```rust
let report = specgate_ctsc::compare("reference.json", "candidate.json", None::<&std::path::Path>, &[]);
println!("{}", report.equivalent);
```

Registry encoding returns opaque contextual errors instead of panicking:

```rust
let schema = specgate_ctsc::registry::Schema::new("not JSON");
let error = specgate_ctsc::registry::encode("example", "1", schema).unwrap_err();
assert!(!error.to_string().is_empty());
```

Byte validation is pure and reports all discovered issues:

```rust
use specgate_ctsc::validation::{DocumentBytes, bytes::validate_trace};
let input = DocumentBytes::new(std::path::Path::new("trace.json"), b"{}");
assert!(!validate_trace(input).valid);
```

Replay decoding verifies manifest digests and linkage before returning instructions:

```rust
assert!(specgate_ctsc::replay::decode(b"{}", b"{}", b"{}").is_err());
```

Capture encoding requires at least one completed scenario and reports invalid construction:

```rust
use specgate_ctsc::capture::{Metadata, Registry, Target, encode_reference};
let metadata = Metadata::new("1", Target::new("target", "rust"), Registry::new("id", "1", "sha256:x"));
assert!(encode_reference([], &metadata).is_err());
```


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

 [__cargo_doc2readme_dependencies_info]: ggGmYW0CYXZlMC43LjNhdIQbJSusbBjLO7EbSlASCvKRTqwbmd2gsLYkxMobU3WiDiuhvKthYvRhcoQbjwwNeiA1CWobPWXemSEYo2kbsTC6M8G5ez4bF96Ajh4YvH1hZIGDbXNwZWNnYXRlLWN0c2NlMC42LjBtc3BlY2dhdGVfY3RzYw
 [__link0]: https://docs.rs/specgate-ctsc/0.6.0/specgate_ctsc/capture/index.html
 [__link1]: https://docs.rs/specgate-ctsc/0.6.0/specgate_ctsc/registry/index.html
 [__link2]: https://docs.rs/specgate-ctsc/0.6.0/specgate_ctsc/replay/index.html
 [__link3]: https://docs.rs/specgate-ctsc/0.6.0/specgate_ctsc/validation/index.html
 [__link4]: https://docs.rs/specgate-ctsc/0.6.0/specgate_ctsc/comparison/index.html
