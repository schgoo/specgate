# Operation-scoped output type bindings

> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#156](https://github.com/schgoo/specgate/issues/156),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

One reference type may correspond to different native candidate types at
different factory boundaries. Assigning them one semantic name creates
duplicate owners. Current replay also publishes converted candidate values
against the reference registry; that cannot faithfully represent missing
candidate fields or distinct native output declarations.

## Decision

**Requested:** approve one shared architecture for #154, #155, and #156:
**two native registries, two faithful traces, one checked correspondence plan.**

| Boundary | Rule |
|---|---|
| Configuration | Put explicit, directed reference-to-candidate output correspondences in the existing component/implementation binding configuration, not native annotations or CTSC type aliases. |
| Selector | Exact component, operation, result role, and typed path segments. Bind native type identities/owners; reject missing, overlapping, stale, or ambiguous selectors. |
| Publication | On the explicitly mapped path, publish the candidate's actual values and native registry closure. Its trace names that registry, not the reference registry. Preserve reference bytes. |
| Provenance | Extend #135 linkage with both registry identities/digests, artifact identities, policy version, and effective bindings. Compare verifies them; current configuration alone is not evidence. |
| Assessment | Check each trace against its own registry. Compare signatures separately from observed values; preserve all violations and incomplete-stimulus findings. |
| Equality | Build typed, deterministic comparison views from each native tree. Compare semantic values before rendering diagnostics; never equate shortened paths or display strings. |

Propose a separate `specgate.mapped/0.1.0` comparison policy, recorded with its
effective bindings. Existing shared-registry comparison and CTSC Strict remain
unchanged. Equal mapped values do not erase different native signatures or
establish full API equivalence.

For example, the `geography` result may bind reference `CoordinateSystem` to
candidate `GeographyCrs`, while `geometry` binds it to `GeometryCrs`. Each
candidate declaration keeps its unique owner. One boundary's mapping does not
apply to another operation or component.

## Consequences

- Initial scope: result outputs and supported nested result paths. Input
  reconstruction, setup/receiver selection, arbitrary renames, and error-channel
  mappings are excluded; ambiguous later native type selection still rejects.
- Preserve existing checked storage conversions as derived views. Do not copy
  reference values, rewrite native evidence, default fields, merge identities,
  deduplicate collections, or infer conversions from observed contents.
- Depends on #134 own-registry findings, #135 versioned linkage/reports, and
  #137 signature comparison. Pending wrapper work is not an available baseline.
- Reject global type aliases and relaxed owner uniqueness. Authored canonical
  adapters remain valid; these explicit mappings are a new alternative.
- Binding grammar, report/API integration, and migration must be reviewed with
  their schemas before implementation. This record approves no invented syntax.

## Validation

Two factories with one reference type and distinct candidate types must replay
and compare independently. Test exact component scoping, changed shared values,
invalid selectors, duplicate owners, registry/link tampering, and unchanged
ordinary comparison. Published candidate bytes must retain native fields,
wrappers, collection laws, and registry linkage.

Implementation seams: binding schema/loader; replay link, storage, execution,
and publication; separate comparator registry contexts and versioned reports.
Extend existing CTSC tests, add honest matrix evidence, regenerate intended
goldens, and run `just check`.

## Supersedes / superseded by

If accepted and implemented, narrowly amends reference-registry publication and
the no-binding-language restriction in
[typed replay](typed-async-client-replay.md#canonical-adapters-and-structured-inputs).
[Missing fields](missing-candidate-output-fields.md) and
[optional output bindings](required-to-optional-output-bindings.md) use this
same architecture, not separate mapping formats.
