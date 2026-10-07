# Specgate-Annotations-Macros

[![crates.io](https://img.shields.io/crates/v/specgate-annotations-macros.svg)](https://crates.io/crates/specgate-annotations-macros)
[![docs.rs](https://docs.rs/specgate-annotations-macros/badge.svg)](https://docs.rs/specgate-annotations-macros)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Annotation macros that connect native Rust code to the `SpecGate` CTSC runtime.

[`spec_operation`][__link0] marks functions and methods as captured operation boundaries,
while [`spec_setup`][__link1] marks deterministic producers used to construct operation
inputs or receivers. Both attributes preserve the annotated item’s behavior and
emit hygienic link-time metadata consumed by discovery. Components are selected
with the facade’s `spec_component!` macro; event projection is provided by the
facade’s `SpecEvent` derive and `spec_trace!` macro.

Most applications import these established names from `specgate`; this
implementation-facing crate exists so the facade can re-export the attributes
without changing generated runtime identity.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

 [__cargo_doc2readme_dependencies_info]: ggGmYW0CYXZlMC43LjNhdIQbJSusbBjLO7EbSlASCvKRTqwbmd2gsLYkxMobU3WiDiuhvKthYvRhcoQbLFEEr5mXHkwbNNRDzNbJlEwbQTp_kLXjQ_IbIOzQH5nm6lRhZIGDeBtzcGVjZ2F0ZS1hbm5vdGF0aW9ucy1tYWNyb3NlMC42LjB4G3NwZWNnYXRlX2Fubm90YXRpb25zX21hY3Jvcw
 [__link0]: https://docs.rs/specgate-annotations-macros/0.6.0/specgate_annotations_macros/attr.spec_operation.html
 [__link1]: https://docs.rs/specgate-annotations-macros/0.6.0/specgate_annotations_macros/attr.spec_setup.html
