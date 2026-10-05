# Templated URI dependency for registry imports

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-01

## Context

The CTSC registry contract permits an optional URI retrieval hint and uses a
relative `file:` URI in its normative example. SpecGate automatically resolves
only local file hints; non-file hints are ignored and network file authorities
are rejected with guidance to use an explicit import.

The general-purpose `url` dependency provided parsing and filesystem-path
conversion. The approved workspace dependency is now the public crates.io
`templated_uri` 0.6 line. Its `Uri` type is intentionally HTTP-oriented and
rejects valid CTSC forms such as `file:./tax.ctsc-registry.json`, so using that
type directly would narrow the contract.

## Decision

- Replace the workspace `url` dependency with
  `templated_uri = { version = "0.6.0", default-features = false }`.
- Keep a file-specific registry-import adapter rather than interpreting CTSC
  file hints as HTTP request URIs.
- Use `templated_uri::Authority` to validate any authority before applying the
  existing localhost-only policy.
- Preserve relative resolution against the importing registry, percent-decoded
  UTF-8 paths, platform absolute-path conversion, canonicalization when the
  target exists, and all existing network, UNC, query, and fragment diagnostics.
- Do not add network retrieval or change the public CLI.

## Consequences

`templated_uri` is the only direct URI dependency. SpecGate does not use its
HTTP-focused `Uri` parser for registry file hints, because doing so would reject
contract-valid input. The dedicated adapter remains deliberately limited to
the retrieval behavior SpecGate already supports.

## Validation

Focused validator tests cover non-file hints, relative and absolute file hints,
percent decoding, localhost authorities, remote authorities, slash and
backslash UNC forms, query and fragment rejection, malformed authorities, and
non-UTF-8 paths. Existing registry validation tests cover automatic import
resolution and unchanged diagnostics.

## Supersedes / superseded by

None.
