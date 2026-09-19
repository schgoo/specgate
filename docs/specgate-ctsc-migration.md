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
  -> validate and compare with ctsc.strict/0.1.0
```

## Retained architecture

- `specgate-runtime`: native inputs, observations, result/empty/error/fault
  completion, deterministic sidecars, and link-time metadata.
- `specgate`: the sole published Rust annotation facade.
- `specgate-discovery`: one strict binding resolver, Rust link-time discovery,
  C# compiled-assembly reflection, raw native invocation metadata, normalized
  DTOs, component-scoped setup folding, and transitive dependency closure.
- `specgate-ctsc`: registry and native OTLP encoding, reusable CTSC Registry,
  Trace Core, Linked, and bundle validation, typed replay decoding,
  deterministic IDs, and strict differential comparison.
- `specgate-cli`: `discover`, `capture`, `replay`, `validate`, and `compare`.

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
  mappings, and C# replay are not implemented.
- Async metadata is retained for linking, but native capture rejects async
  operations before polling until capture context can propagate task-safely.
- Native validation supports JSON and JSONL traces, registry imports, exact
  capture-bundle integrity, and linked type checking. Bundle validation is
  intentionally independent from replay's narrower invocation decoder.
- Differential comparison currently implements the fixed
  `ctsc.strict/0.1.0` policy. Multiple-run selection, overlapping sequential
  children, and duplicate parallel branch identities are rejected as
  unsupported or ambiguous, as permitted by the policy.
- Replay validates nested operations as observed behavior but invokes only
  top-level reference operations.
