# SpecGate decision records

Use this directory for durable, human-ratified architectural decisions.
Decision records complement the normative CTSC contracts; they do not override
those contracts unless the same change updates the contract.

Do not create retrospective records that imply approval which did not occur.
Create a record when a named owner resolves a load-bearing choice, then link it
from the affected contract, status document, or digest.

## ADR numbering

Every architecture decision record (ADR), including a proposal, has one
repository-wide `ADR-NNNN` identifier in its metadata. Refer to a decision by
that identifier, for example **ADR-0022**, regardless of its filename or status.

- Allocate the next unused number when adding a record. Never renumber or reuse
  an identifier, including when a proposal is rejected or superseded.
- Add the record to the index below. Resolve identifier collisions between
  branches before merging; keep existing identifiers fixed.
- Keep filenames and headings stable so existing links and anchors still work.
  An identifier is not an approval or implementation claim.

The initial historical allocation follows each record's stated date, with
filename order breaking ties. The undated collection-dimensions record is
placed using its first commit date, 2026-11-03; no approval date is inferred.
The seven roadmap proposals follow issue order after the historical records.
Future records always append, even when documenting an older decision.

## Decision index

| ADR | Decision |
|---|---|
| [ADR-0001](async-capture-context.md) | Async capture context |
| [ADR-0002](native-capture-finalization.md) | Native capture finalization |
| [ADR-0003](rust-semantic-identities-and-capture-api.md) | Rust semantic identities and capture API |
| [ADR-0004](templated-uri-for-registry-imports.md) | Templated URI dependency for registry imports |
| [ADR-0005](abandonment-terminal-state.md) | Abandonment as a terminal state |
| [ADR-0006](opaque-rust-errors.md) | Opaque Rust errors |
| [ADR-0007](top-level-abandonment-unsupported.md) | Top-level async abandonment is unsupported |
| [ADR-0008](csharp-capture-instrumentation.md) | C# capture instruments the built assembly by IL weaving |
| [ADR-0009](binding-schema-v1.md) | Component-to-implementation binding |
| [ADR-0010](csharp-async-capture.md) | C# async capture |
| [ADR-0011](csharp-capture-cli.md) | C# capture runs native tests through the CLI |
| [ADR-0012](csharp-golden-replay-parity.md) | Native C# goldens and implementation-to-implementation replay checks |
| [ADR-0013](typed-async-client-replay.md) | Typed generic operations and setup-backed async replay |
| [ADR-0014](constructor-and-property-setups.md) | Constructor and property setups |
| [ADR-0015](null-values-are-capture-diagnostics.md) | Null values are capture diagnostics |
| [ADR-0016](registry-guided-collection-replay.md) | Collection storage bindings in registry-driven replay |
| [ADR-0017](collection-dimensions.md) | Collection shape and semantic dimensions |
| [ADR-0018](value-decomposition.md) | Language-neutral value decomposition |
| [ADR-0019](custom-collection-metadata.md) | Compiler-directed custom collection metadata |
| [ADR-0020](prebuilt-csharp-discovery-tool.md) | Prebuilt C# discovery tool |
| [ADR-0021](rust-component-selector.md) | Rust component-selector spelling |
| [ADR-0022](csharp-operation-setup-factories.md) | C# factories as operations and setups |
| [ADR-0023](partial-replay-coverage.md) | Partial replay with explicit coverage |
| [ADR-0024](missing-candidate-output-fields.md) | Missing candidate output fields |
| [ADR-0025](required-to-optional-output-bindings.md) | Required-to-optional output bindings |
| [ADR-0026](operation-scoped-type-bindings.md) | Operation-scoped output type bindings |
| [ADR-0027](copy-receiver-replay.md) | Replay of Copy by-value receivers |
| [ADR-0028](named-enum-replay-inputs.md) | Named enum replay inputs |

## File naming

Use a short, stable, kebab-case name, for example:

```text
docs/decisions/async-capture-context.md
```

## Roadmap 159 architecture change records

These ADRs propose the fixes in [roadmap #159](https://github.com/schgoo/specgate/issues/159).
They follow the decision-record format below. **Proposed is not approved or
implemented.** Each record separates its decision, compatibility boundary,
tradeoffs, and acceptance evidence.

| ADR | Issue | Record | Main decision |
|---|---|---|---|
| ADR-0022 | [#152](https://github.com/schgoo/specgate/issues/152) | [Dual-role C# factories](csharp-operation-setup-factories.md) | Finish the factory operation before publishing setup inputs. Existing attribution/scope approved; lifecycle pending. |
| ADR-0023 | [#153](https://github.com/schgoo/specgate/issues/153) | [Partial replay](partial-replay-coverage.md) | Explicitly skip whole unsupported scenarios; retain coverage and inconclusive results. |
| ADR-0024 | [#154](https://github.com/schgoo/specgate/issues/154) | [Missing output fields](missing-candidate-output-fields.md) | Missing fields reach comparison, not replay rejection. Direction approved; binding/policy design pending. |
| ADR-0025 | [#155](https://github.com/schgoo/specgate/issues/155) | [Required-to-optional outputs](required-to-optional-output-bindings.md) | Permit an explicit lossless output binding; retain signature differences. |
| ADR-0026 | [#156](https://github.com/schgoo/specgate/issues/156) | [Operation-scoped type bindings](operation-scoped-type-bindings.md) | Preserve native registries and unique owners; bind exact output boundaries. |
| ADR-0027 | [#157](https://github.com/schgoo/specgate/issues/157) | [Copy receivers](copy-receiver-replay.md) | Require explicit intent and compiler-proven copying of the retained receiver. |
| ADR-0028 | [#158](https://github.com/schgoo/specgate/issues/158) | [Named enum inputs](named-enum-replay-inputs.md) | Any variant form uses typed recorded inputs or explicit setup producers. Principle approved; nested setup design pending. |

## Template

```markdown
# Decision title

> **ADR:** ADR-NNNN
> **Status:** proposed | accepted | superseded
> **Owner:** person or team
> **Date:** YYYY-MM-DD

## Context

What compatibility, product, or operational problem requires a decision?

## Decision

What was explicitly chosen?

## Consequences

What becomes required, prohibited, deferred, or intentionally unsupported?

## Validation

Which tests, schemas, validators, or artifacts pin the decision?

## Supersedes / superseded by

Links to related records, if any.
```
