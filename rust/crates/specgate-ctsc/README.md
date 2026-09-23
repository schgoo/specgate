# Specgate-Ctsc

[![crates.io](https://img.shields.io/crates/v/specgate-ctsc.svg)](https://crates.io/crates/specgate-ctsc)
[![docs.rs](https://docs.rs/specgate-ctsc/badge.svg)](https://docs.rs/specgate-ctsc)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

CTSC registry, trace, capture-bundle, and replay models for `SpecGate`.

Native captures are encoded directly from real annotated operation
boundaries, preserving nested parentage, typed inputs/results, observations,
empty/error/fault completion, logical timestamps, and deterministic IDs.
Registry encoding consumes normalized discovery metadata while retaining
setup/dependency/outcome information. The crate also provides native CTSC
Registry, Trace Core, Linked, capture-bundle validation, and deterministic
`ctsc.strict/0.1.0` differential comparison.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.