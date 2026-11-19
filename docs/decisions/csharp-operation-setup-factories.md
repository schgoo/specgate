# C# factories as operations and setups

> **ADR:** ADR-0022
> **Status:** proposed; lifecycle approval pending
> **Owner:** @schgoo (attribution and initial scope approved 2026-11-18)
> **Date:** 2026-11-18
> **Tracking:** [#152](https://github.com/schgoo/specgate/issues/152),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

**One C# factory can have two useful roles, but SpecGate cannot record both
roles on that method.**

Why it matters: comparing object creation should not prevent testing later
calls on the created object.

- **Operation:** a call such as `Geography(4326)` creates an object whose
  returned fields are behavior to compare.
- **Setup:** the same call supplies the object for later instance operations.
  SpecGate saves its construction inputs so replay can create a receiver,
  the object on which those methods run.
- **Current limit:** discovery accepts both annotations. The weaver, which
  adds recording code to the compiled method, rejects the combination.
- **Workaround:** moving setup to a separate method that calls the factory
  changes the annotation and test layout. That does not make the original
  factory work in both roles.

## Decision

**Requested:** approve one composed rewrite with this order:

| Boundary | Required behavior |
|---|---|
| Entry | Snapshot setup inputs before running the factory or opening its operation scope. |
| Body | Open the ordinary operation; execute the original body once. Preserve nested-call parentage. |
| Success | Save the actual return value; finish the operation; commit setup inputs; return the same value. |
| Target throw | Record the existing declared error or fault; rethrow the original exception; commit no new setup inputs. |
| Finalization failure | Do not commit setup inputs. Preserve the existing capture failure. |
| Setup commit failure | Reject capture outside the target-exception handler. Do not recast the failure as a factory exception or change its recorded result. |

**Already approved:** retain existing exact declaration/session attribution.
Inputs are associated with component, target operation, producer, and `Fills`,
not returned-object identity. Identical repeats are accepted; different inputs
remain ambiguous. Prior successful provenance survives a later failed call.
Multiple setup declarations keep their current rules; no transactional commit
or "latest factory wins" behavior is added.

## Consequences

- One synchronous, nongeneric method may carry both existing annotations.
  New setup inputs are unavailable inside its body.
- Preserve operation-only/setup-only weaving, ignored parameters, native
  signatures, exception identity, inactive behavior, and reweaving idempotence.
- Exclude async/generic setups and factories requiring their own unpublished
  setup inputs. No CLI, CTSC shape, runtime API, or replay-policy change.
- Compose one method rewrite; applying two whole-method rewrites is rejected
  because it obscures exception boundaries and completion order.

## Validation

Woven ordinary tests cover real results, early returns, catch/finally, nested
calls, exact/multiple targets, ignored inputs, identical/differing repeats,
target throws, commit failures, and inactive/repeated weaving. Discovery must
retain both declarations. A native C# fixture must pass Registry, Trace,
Linked, bundle, and matrix checks; regenerate its goldens.

Implementation seams: `AssemblyWeaver`, `MethodWeaver`, existing `SetupStaging`
and `CaptureSession.RecordSetup`; no runtime change without a new scope review.
Run focused C# checks, mapped docs checks, and `just check`.

## Supersedes / superseded by

Extends the weaving restriction, not the attribution rules in
[constructor and property setups](constructor-and-property-setups.md).
Lifecycle approval and implementation are not implied by this proposed record.
