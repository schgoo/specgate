# SpecGate CTSC Migration Status

## Legacy removal complete

The repository now uses CTSC-native discovery, capture, and replay. The former
spec parser, behavioral matcher, generated runners, case extraction, coverage,
self-hosting, flat event buffer, record-mode JSONL sink, mock runtime, C#
weaver/runtime, and associated product commands have been removed.

The active flow is:

```text
reference target
  -> discover semantic registry
  -> capture ordinary tests at native operation boundaries
  -> verify bundle digests and linkage
  -> statically link top-level stimuli to a candidate
  -> replay
  -> emit an independent candidate CTSC trace
```

## Retained architecture

- `specgate-runtime`: native inputs, observations, result/empty/error/fault
  completion, deterministic sidecars, and link-time metadata.
- `specgate`: the sole published Rust annotation facade.
- `specgate-discovery`: one strict binding resolver, Rust link-time discovery,
  C# compiled-assembly reflection, raw native invocation metadata, normalized
  DTOs, component-scoped setup folding, and transitive dependency closure.
- `specgate-ctsc`: registry and native OTLP encoding, capture-bundle
  digest/link verification, typed replay decoding, and deterministic IDs.
- `specgate-cli`: `discover`, `capture`, and `replay`.

Rust and C# registry output is byte-identical for the stateless, rich-type, and
setup-folding fixtures. Raw language-specific invocation metadata remains
separate from the language-neutral CTSC registry.

Discovery, capture, and replay scratch work uses invocation-unique
operating-system cache directories. Generated runners use the exact
`specgate-runtime` path or registry version resolved from the candidate's
`cargo metadata`; caller working directories are never searched. Advanced
tooling may explicitly override this with `SPECGATE_RUNTIME_PATH` or
`SPECGATE_RUNTIME_VERSION`, but only as an assertion selecting the same Cargo
PackageId. A path must resolve to that exact package identity, while a version
must match the candidate's crates.io package; mismatches fail before runner
generation to prevent split runtime/linkme/capture state.

## Current limitations

- Reference capture supports Rust libtest targets only.
- Candidate replay supports synchronous public Rust free functions with
  lossless primitive inputs.
- Setup-backed methods, async calls, structured replay values, explicit
  mappings, C# replay, and differential comparison are not implemented.
- Async metadata is retained for linking, but native capture rejects async
  operations before polling until capture context can propagate task-safely.
- Native traces support observations and result/empty/declared-error/fault
  completion, but registry observation declarations and comparison profiles are
  future work.
- Replay validates nested operations as observed behavior but invokes only
  top-level reference operations.
