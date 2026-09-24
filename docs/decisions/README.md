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
