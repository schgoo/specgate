# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `specgate` umbrella crate — single dependency for annotations + harness
- `specgate-annotations` — proc macros for `#[spec_operation]`, `#[spec_setup]`, `#[derive(SpecEvent)]`, `spec_trace!`
- `specgate-runtime` — thread-local trace buffer, `Value` type, `ToSpecValue` trait
- `specgate-harness` — test harness: codegen, trace collection, subsequence matching
- `specgate-cli` — `specgate validate` and `specgate run` commands
- Spec schema v0.4.0 with operations, types, structured values, property tests
- Assertion operators: `$eq`, `$size`, `$contains`, `$containsAll`, `$excludes`, `$match`, `$exists`, `$any`, `$type`, `$matches`, `$not`, `$gt`/`$gte`/`$lt`/`$lte`, `$every`
- `$run`, `$unordered`, `$anywhere` trace directives
- Multi-target binding with per-case target override
- Enum `derive(SpecEvent)` — unit and named-field variants
- Complex input deserialization via serde (structs, enums, lists, maps, optionals)
- `ToSpecValue` impl for structs and enums via `derive(SpecEvent)`
- Property test syntax (`kind: property`, generators, calls, `$assert`)
- `specgate-trace` feature flag for zero-cost annotations in release builds
- Level/source/async support on operations and cases
- CLI validate: 14 static checks (schema, ops, inputs, deps, narratives, source visibility)
- Dual license: MIT OR Apache-2.0
- CI: GitHub Actions (build, test, clippy, fmt, deny) on ubuntu + windows
