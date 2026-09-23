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
records native observations. Async operations retain metadata but reject
native capture before polling, and an async setup records no construction
inputs at all, so capture rejects a component that declares one.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.