# SpecGate

[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](./LICENSE-MIT)

Deterministic spec-based verification for LLM-implemented code.

Engineers write specs. LLMs implement them. SpecGate closes the gap by providing
a non-stochastic harness that validates implementations against specs using
runtime traces.

## How It Works

```
Spec YAML  +  Annotated Source  →  Harness  →  Pass / Fail per case
```

1. Write a spec (`.spec.yaml`) declaring operations, types, and expected behavior
2. Annotate the source so operations, state, and inline checkpoints emit traces
3. The harness generates a runner, compiles it, collects traces, and compares

See the [`specgate` crate docs](./rust/crates/specgate/README.md) for usage,
and the [CLI docs](./rust/crates/specgate-cli/README.md) for command-line usage.

## What SpecGate can do

SpecGate verifies that code does what a spec says by comparing the code's
**runtime traces** to the spec's expectations — deterministically, with no LLM
in the verification loop. It runs in **both directions**: verify an
implementation against a spec, or **extract** a spec from annotated code.

The capability areas below each link to reference docs, and every one has a
canonical runnable example in the [Fixture Catalog](./docs/knowledge/fixtures.md):

- **Operations & trace assertions** — declare a component's API and assert on the
  events its operations emit. ([spec-format](./docs/knowledge/spec-format.md))
- **Rich trace matching** — subsequence matching plus MongoDB-aligned operators
  (`$gt`, `$contains`, `$match`, `$type`, `$not`, …) and position directives
  (`$unordered`, `$anywhere`).
  ([operators](./docs/knowledge/spec-format.md#complete-operator-catalog))
- **State, setups & mocks** — stateful operations, receiver construction, and
  dependency mocking.
  ([construction](./docs/knowledge/construction.md),
  [annotations](./docs/knowledge/annotations.md))
- **Structured & typed values** — structs, enums, lists/maps/sets, optional
  inputs with defaults, and the built-in `value` type.
  ([spec-format](./docs/knowledge/spec-format.md))
- **Property-based cases** — run an operation over generated inputs and assert
  invariants, reporting a counterexample on failure.
  ([spec-format](./docs/knowledge/spec-format.md))
- **Async operations** — driven on a per-target async runtime selected in the
  binding. ([bindings](./docs/knowledge/bindings.md#async-runtime))
- **Components** — group a crate's public API and relate components via
  `depends_on`. ([annotations](./docs/knowledge/annotations.md))
- **Spec extraction** — derive a spec from annotated code, schema-only or with
  captured test cases. ([extract](./docs/knowledge/extract.md))

Behavior is documented by example: the
**[Fixture Catalog](./docs/knowledge/fixtures.md)** maps every capability to a
runnable fixture — those fixtures are the source of truth.

## CLI

The `specgate` CLI has three commands:

```bash
# Validate specs in a directory (schema + runnability checks)
specgate validate <spec-dir> [--strict] [--spec-only] [--assertions-dir <dir>]

# Run a spec through the harness (optionally with coverage)
specgate run <spec.yaml> [--coverage] [--coverage-threshold <pct>]

# Extract a spec from an annotated crate (reverse direction)
specgate extract <package-root> -o|--out <spec.yaml> [--component <name>] [--cases]
```

- `validate` — checks specs against the schema plus runnability checks that
  mirror the harness's hard errors. `--spec-only` skips checks needing the
  implementation source; `--strict` treats warnings as errors;
  `--assertions-dir` points at a directory of source-assertion files.
- `run` — generates, builds, and runs the harness for a spec, reporting
  per-case pass/fail. `--coverage` measures implementation coverage;
  `--coverage-threshold <pct>` fails the run below the given percentage.
- `extract` — derives a spec (and sibling binding) from annotated code. See
  [extract.md](./docs/knowledge/extract.md).

## Design Principles

1. **Spec is the single source of truth**
2. **Conformance checking is deterministic** — no LLM in the verification loop
3. **Traces are the evidence** — generated tests have zero domain knowledge
4. **Zero-cost in production** — annotations compile to no-ops in production builds

## Documentation

- [`docs/knowledge/`](./docs/knowledge/index.md) — reference docs: spec format,
  annotations, bindings, targets, validation, extraction, and more
- [**Fixture Catalog**](./docs/knowledge/fixtures.md) — every feature
  demonstrated by a runnable fixture (the canonical, source-of-truth examples)
- [`CHANGELOG.md`](./CHANGELOG.md) — release history

### Implementation Skill (for AI agents)

[`.github/skills/implement-spec.md`](./.github/skills/implement-spec.md) is a structured
workflow that AI coding agents follow when implementing from a spec. It defines:

- **How to read a spec** — what each field means, how types map to code
- **The TDD workflow** — annotate first, write harness bootstrap test, then implement
- **Validation gates** — `specgate validate` before implementing, `specgate run` after
- **Trust boundary** — agents must never read harness-generated artifacts as implementation input
- **Checklist** — what must be true before an implementation is considered done

Point any agent at this file when asking it to "implement a spec" or "build from spec".
The knowledge base in `docs/knowledge/` provides topic-specific reference (spec format,
annotation syntax, binding files, Rust/C# conventions, etc.).

## Roadmap

Directions we're working toward (not commitments), roughly nearest-term first.
Several are tracked as [GitHub issues](https://github.com/schgoo/specgate/issues).

- **Deeper extraction** *(next up)* — capture `--cases` for methods and state
  machines (today's `--cases` covers free functions).
- **C# backend** — the annotation surface, harness, and extraction for C#.
  SpecGate is Rust-only today; the intended C# surface is sketched in
  [`csharp.md`](./docs/knowledge/csharp.md).
- **Cross-implementation comparison** — run one spec against multiple
  implementations and compare conformance (e.g. a reference vs. a candidate, or
  a Rust vs. C# implementation of the same spec).
- **Richer validation & conformance reports** — structured, diffable/replayable
  reports beyond per-case pass/fail, including shared assertions across cases
  ([#17](https://github.com/schgoo/specgate/issues/17)) and fixing narrative
  cases that inflate the pass count
  ([#11](https://github.com/schgoo/specgate/issues/11)).
- **Registry as the single source of truth** — migrate `run` and `validate` off
  source-scanning onto the link-time registry that `extract` already uses
  ([#25](https://github.com/schgoo/specgate/issues/25)).
- **Annotation ergonomics** — disambiguate `#[spec_operation]`'s dual role
  ([#10](https://github.com/schgoo/specgate/issues/10)) and add spec-level
  signals that encourage good API design
  ([#23](https://github.com/schgoo/specgate/issues/23)).
- **Binding execution context** — per-target `cwd`/`env`
  ([#18](https://github.com/schgoo/specgate/issues/18)).
- **Observable globals** — a wrapper type (e.g. `SpecCell<T>`) whose accessor
  methods emit trace events, so a global/ambient value can be declared observed
  once at its definition and traced on every mutation — instead of an inline
  `spec_trace!` at each site. (Establishing global state already works via a
  side-effect setup with a `setup:` input; this covers observing it.)
- **Concurrent trace isolation** — scoped trace buffers per operation
  ([#2](https://github.com/schgoo/specgate/issues/2)).
- **Inspectable run artifacts** — retain the generated runner/scratch dir for
  debugging ([#13](https://github.com/schgoo/specgate/issues/13)).

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

[MIT](./LICENSE-MIT) OR [Apache-2.0](./LICENSE-APACHE)
