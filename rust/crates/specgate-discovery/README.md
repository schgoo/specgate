# Specgate-Discovery

[![crates.io](https://img.shields.io/crates/v/specgate-discovery.svg)](https://crates.io/crates/specgate-discovery)
[![docs.rs](https://docs.rs/specgate-discovery/badge.svg)](https://docs.rs/specgate-discovery)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Native implementation discovery for `SpecGate`’s CTSC workflow.

This crate owns the strict target-binding resolver, Rust link-time metadata
discovery, C# compiled-assembly reflection discovery, raw invocation
metadata, and deterministic semantic schema normalization. It deliberately
has no dependency on `.spec.yaml`, cases, runners, matching, coverage, or
reports. Generated runners use invocation-unique operating-system cache
directories and select verified local workspace dependencies only when
available, otherwise using the compatible published crate version.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.