# Specgate-Annotations-Macros

[![crates.io](https://img.shields.io/crates/v/specgate-annotations-macros.svg)](https://crates.io/crates/specgate-annotations-macros)
[![docs.rs](https://docs.rs/specgate-annotations-macros/badge.svg)](https://docs.rs/specgate-annotations-macros)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Native CTSC annotation macros for `SpecGate`.

Operations create real capture boundaries; setups register link-time
metadata and record the construction inputs the registry folds into the
operation they build; types register raw link-time metadata; `SpecEvent`
projects structured values through `ToNativeValue`; and `spec_trace!`
records native observations. An async operation captures the caller’s
operation where its future is *constructed* and begins recording at its
first poll, so it records under the operation that built it even when
another thread resumes it; it completes when the body’s future resolves. An
async setup records no construction inputs at all, so capture rejects a
component that declares one.

To run code at construction time, `#[spec_operation]` rewrites an annotated
`async fn` into a `fn` returning `impl Future<Output = T>`. Three
consequences are worth knowing:

* **Edition 2024 only.** The rewritten signature relies on edition 2024’s
  rule that an RPIT captures every in-scope lifetime. On edition 2021 a
  borrowing `async fn` would need an explicit `use<..>` bound, which is not
  emitted.
* **The returned future is boxed once.** The wrapper owns a `Pin<Box<_>>` of
  the body so it needs no pin projection, which `unsafe_code = "forbid"`
  would otherwise make impossible without a new dependency.
* **No auto-trait bound is declared.** The rewrite emits a bare
  `impl Future<Output = T>`, so `Send`, `Sync`, and the rest leak from the
  body exactly as they do for an un-annotated `async fn`. Declaring `+ Send`
  would reject bodies that hold a non-`Send` value across an `.await`.

The registry is unaffected: `return_type`, `invocation`, `return_kind`, and
`is_async` are authored from the signature as the caller wrote it, before
the rewrite, so byte-identical parity with the C# twin is preserved.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.