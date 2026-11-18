# SpecGate decision records

Use this directory for durable, human-ratified architectural decisions.
Decision records complement the normative CTSC contracts; they do not override
those contracts unless the same change updates the contract.

Do not create retrospective records that imply approval which did not occur.
Create a record when a named owner resolves a load-bearing choice, then link it
from the affected contract, status document, or digest.

## File naming

Use a short, stable, kebab-case name, for example:

```text
docs/decisions/async-capture-context.md
```

## Roadmap 159 architecture change records

These ACRs propose the fixes in [roadmap #159](https://github.com/schgoo/specgate/issues/159).
They follow the decision-record format below. **Proposed is not approved or
implemented.** Each record separates its decision, compatibility boundary,
tradeoffs, and acceptance evidence.

| Issue | Record | Main decision |
|---|---|---|
| [#152](https://github.com/schgoo/specgate/issues/152) | [Dual-role C# factories](csharp-operation-setup-factories.md) | Finish the factory operation before publishing setup inputs. Existing attribution/scope approved; lifecycle pending. |
| [#153](https://github.com/schgoo/specgate/issues/153) | [Partial replay](partial-replay-coverage.md) | Explicitly skip whole unsupported scenarios; retain coverage and inconclusive results. |
| [#154](https://github.com/schgoo/specgate/issues/154) | [Missing output fields](missing-candidate-output-fields.md) | Compare shared fields without inventing absent candidate values. |
| [#155](https://github.com/schgoo/specgate/issues/155) | [Required-to-optional outputs](required-to-optional-output-bindings.md) | Permit an explicit lossless output binding; retain signature differences. |
| [#156](https://github.com/schgoo/specgate/issues/156) | [Operation-scoped type bindings](operation-scoped-type-bindings.md) | Preserve native registries and unique owners; bind exact output boundaries. |
| [#157](https://github.com/schgoo/specgate/issues/157) | [Copy receivers](copy-receiver-replay.md) | Require explicit intent and compiler-proven copying of the retained receiver. |
| [#158](https://github.com/schgoo/specgate/issues/158) | [Named enum inputs](named-enum-replay-inputs.md) | Reconstruct complete unit-variant enums in their native lexical scope. |

## Template

```markdown
# Decision title

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
