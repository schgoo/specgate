# Specgate-Ctsc

[![crates.io](https://img.shields.io/crates/v/specgate-ctsc.svg)](https://crates.io/crates/specgate-ctsc)
[![docs.rs](https://docs.rs/specgate-ctsc/badge.svg)](https://docs.rs/specgate-ctsc)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

CTSC projection for `SpecGate` — encodes native structured operation
capture and provides transitional translation for legacy flat traces.

Native synchronous Rust capture now creates operation spans at real
`#[spec_operation]` invocation boundaries, including nested parentage,
typed inputs and results, observations, logical timestamps, and status. Its
public producer operations expose CTSC artifacts only.
Native input/result projection is type-aware and recursively preserves CTSC
0.2 option wrappers inside supported collections and annotated records.
Ordered native sidecars can be merged into one deterministic run with
registry identity, version, and digest resource attributes for Linked
validation.
Capture bundles can also be decoded through [`decode_replay_bundle_result`][__link0],
which verifies manifest format/version, exact registry and trace digests,
trace registry linkage, scenario order, operation declarations, and typed
primitive inputs before returning ordered [`ReplayScenario`][__link1] values. The
decoder exposes only top-level operations as replay instructions. Candidate
native captures use [`encode_replayed_native_captures_otlp_result`][__link2] to retain
the original registry identity while using deterministic IDs independent
from the reference run.

The legacy `Run`/`Event` buffer and the translation operations below remain
temporarily for extraction and harness subsystems that do not yet have CTSC
replacements. They are not a stable compatibility surface.

`translate_legacy_trace` walks a JSON-encoded sequence of legacy
[`specgate_runtime::TraceEvent`][__link3]s — a leading `Run` event followed by
ordinary `Event`s — and re-projects it into a [`CtscProjection`][__link4]:

* the leading `Run` event supplies `operation_name`;
* an event named `<operation_name>.<field>` is un-prefixed and becomes an
  operation input, keyed by `<field>`;
* every other ordinary event becomes an observation, keyed by its own name;
* the reserved `$result` / `$fault` event names select the terminal
  `completion` state (`"result"`, `"fault"`, or `"none"` if neither
  appears); that event’s value becomes `completion_value_json`.

Values keep their [`specgate_runtime::Value`][__link5] shape as-is: an event whose
value happens to look like `{"Integer": 7}` is a genuine single-entry map,
not a legacy tagged scalar, and is projected unchanged.

`encode_legacy_trace_otlp` applies the same projection and emits one compact,
deterministic CTSC 0.2 OTLP JSON document containing a run span, its
scenario child, and one operation child. Caller-supplied identifiers,
timestamp, tool version, and target metadata make production identity
explicit while keeping tests reproducible.

`encode_discovery_registry` preserves the original raw-discovery projection
for primitive operations. `encode_schema_registry` accepts the harness’s
normalized, setup-folded schema and emits named records, tagged unions, and
recursive CTSC collection/option references without reimplementing
language-specific discovery or normalization.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

 [__cargo_doc2readme_dependencies_info]: ggGmYW0CYXZlMC43LjNhdIQbmReN9dOqGMIb9otqWGRls0MbbA5gMtcxfWobm07iTKD86xFhYvRhcoQbZhaoKJrgiRIbuV-qIphK598brBfd4ioKWu0b-3fqEUahkYxhZIKDbXNwZWNnYXRlLWN0c2NlMC41LjBtc3BlY2dhdGVfY3RzY4Nwc3BlY2dhdGUtcnVudGltZWUwLjUuMHBzcGVjZ2F0ZV9ydW50aW1l
 [__link0]: https://docs.rs/specgate-ctsc/0.5.0/specgate_ctsc/fn.decode_replay_bundle_result.html
 [__link1]: https://docs.rs/specgate-ctsc/0.5.0/specgate_ctsc/struct.ReplayScenario.html
 [__link2]: https://docs.rs/specgate-ctsc/0.5.0/specgate_ctsc/fn.encode_replayed_native_captures_otlp_result.html
 [__link3]: https://docs.rs/specgate-runtime/0.5.0/specgate_runtime/?search=TraceEvent
 [__link4]: https://docs.rs/specgate-ctsc/0.5.0/specgate_ctsc/struct.CtscProjection.html
 [__link5]: https://docs.rs/specgate-runtime/0.5.0/specgate_runtime/?search=Value
