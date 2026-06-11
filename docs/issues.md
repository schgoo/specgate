# SpecGate Open Issues

## Per-case build configurations

Specs are language-agnostic but may need cases that build with different
configurations (features, flags, compile-time options). The binding file
controls the build command, but it's one command for all cases.

Need a way for cases to express semantic config dimensions (e.g.,
`async: true`) that bindings map to language-specific flags. Deferred
until we have a concrete use case.

## fundle crate for construction resolution

The harness resolves a construction graph (work backwards from entry point,
find constructors/setups for each type). This is conceptually DI resolution.
The `fundle` crate could potentially help — either as a conceptual model
or as runtime wiring in generated test code. Deferred until the harness
is being built.
