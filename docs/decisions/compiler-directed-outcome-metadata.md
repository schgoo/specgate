# Compiler-directed Rust type and outcome metadata

> **ADR:** ADR-0029
> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-20
> **Tracking:** [#122](https://github.com/schgoo/specgate/issues/122),
> [#116](https://github.com/schgoo/specgate/issues/116)

## Context

**Rust should tell SpecGate which type an operation uses, even when its source
signature uses an alias. Discovery and capture must use the same answer.**

Why it matters: an ordinary API alias can prevent discovery or select the wrong
recorded outcome.

- **Current limit:** Rust discovery interprets stringified signatures. Imported
  aliases can hide a result's success and error types. `Self` also lacks its
  enclosing type in the current method registration.
- **Two consumers:** capture separately guesses outcome channels from the final
  spelling `Result` or `Option`. Fixing discovery alone would leave capture
  wrong. An unrelated record with one of those names must remain a record.
- **Accepted direction:** ADR-0019 extends `SpecEvent` with static metadata and
  compiler-instantiated operation callbacks. It does not settle the public
  method, metadata representation, or full compatibility policy.
- **Proof, not implementation:** an isolated experiment used real annotations,
  callbacks, native capture, and existing Registry, Trace, and Linked validators.
  Parent and independent review accepted its bounded evidence. It bypassed
  production discovery, did not solve `Self`, and did not pass the full gate.

This proposal asks for three decisions: the typed metadata contract, its
compatibility boundary, and a narrow exception to collection delivery order.
The decisions below are requested, not already approved.

## Minimal example

Illustrative Rust source; imports are shown, but ordinary capture tests are
omitted. Alias-aware behavior is proposed, not current product support.

```rust
use specgate::{SpecEvent, spec_component, spec_operation};

spec_component!("example.points");

#[derive(SpecEvent)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

mod api {
    pub type Fallible<T> = core::result::Result<T, String>;
}

use api::Fallible as Outcome;

#[spec_operation("point")]
pub fn point() -> Outcome<Point> {
    Ok(Point { x: 1, y: 2 })
}
```

Under the proposal, discovery declares a successful named `Point` owned by
`example.points` and the existing default `error` with a string payload.
Capture records `Ok` as that result and `Err` as that error. Neither the alias
name `Outcome` nor an observed map key selects the channel.

This does not flatten error enums, expose their variants as separate errors,
or infer a cross-language mapping.

## Decision

**Requested:** use one compiler-resolved metadata contract for Rust type
discovery and capture outcome selection, without a new author-facing trait.

### 1. Typed API on the existing trait

Proposed public method, with `TypeMetadata` exported through the existing
`specgate` facade:

```rust
pub trait SpecEvent: ToNativeValue {
    fn type_metadata() -> Option<TypeMetadata>
    where
        Self: Sized,
    {
        None
    }
}
```

`Some` supplies a typed description; `None` means unavailable, not an unknown
type that discovery may guess. `Self: Sized` preserves trait-object use.
Record/union derives generate the method. Supported standard types use the
same method. Manual serializers can supply metadata by implementing this
existing trait; `ToNativeValue` stays responsible only for actual projection.

`TypeMetadata` is a runtime-owned Rust model, not JSON, a CTSC wire document,
or a parsed `type_name` string. Its required distinctions are:

| Metadata | Meaning |
|---|---|
| Primitive/unit | The existing semantic primitive. |
| Standard `Option` / `Result` | The actual Rust constructors, with typed payload descriptions. Aliases resolve to these constructors. |
| Transparent native wrapper | A supported reference or `Box` preserves its semantic value without promoting an enclosed outcome to the operation's outer channel. |
| Named declaration reference | The compiler-selected record/union declaration and its owner, with a separately obtained definition. |
| Tuple/collection/map | Existing semantic shape, including declared collection/map laws. No inference from observed values. |

Keep outcome constructors distinct from ordinary named unions until the
operation boundary is classified. Do not equate every tagged union containing
`Ok`, `Err`, `Some`, or `None` with a standard wrapper.

Named references must not eagerly expand their fields. Resolve definitions
through declaration providers and track visited declarations, so ordinary
named cycles terminate. Reuse an existing named reference on a back-edge;
reject endlessly expanding inline metadata rather than overflowing the stack.
Visited state must distinguish concrete instantiations, not just declaration
templates; seeing the same template again is not by itself a cycle.
Retain duplicate-owner detection even when two declarations have equal shapes.

This fixes the method and representation direction. Exact descriptor
constructors and internal storage layout require implementation review; no
new collection-authoring syntax is approved here.

### 2. One source, separate consumers

The operation macro registers callbacks for concrete input and output types.
For explicitly declared generic specializations, generate these callbacks
outside the authored generic body. Do not add a discovery-only `SpecEvent`
bound or a new `'static` requirement to that body or its borrowed values.
Ignored parameters need no semantic metadata or projection.

```text
compiler-instantiated callback
             |
       typed metadata
        /          \
discovery          capture
registry types     outer outcome plan
```

Discovery follows the descriptors at supported use sites: operation inputs
and outputs, record/union members, and setup surfaces. It applies the existing
normalization and setup-folding rules. Raw native invocation information still
exists where runners need it; it does not decide semantic type identity.

Capture uses the same resolved outer descriptor, not a second signature
parser. Return projection remains borrowed and lazy: no metadata lookup or
serialization for an inactive operation, and one return projection when an
active completion needs a value. Reject unavailable required completion
metadata before projecting that return. Inspect projected tags only after
the descriptor selects the expected standard wrapper; never infer a channel
from the tag itself.

SpecGate must not construct specimen values or invoke `ToNativeValue` while
obtaining metadata. Author-written metadata providers must obey that contract;
the proof is not a general purity checker.

### 3. Preserve identity and current outcomes

A named reference retains its compiler-selected declaration, semantic name,
and component. Discovery must connect that declaration to its authoritative
registry owner. Existing CTSC import resolution supplies version/digest checks,
not automatic native-declaration ownership: the current binding model does
not supply an external-registry mapping. That integration remains required
work, not a capability established by the proof.

Do not hard-code the experiment's local registry, search all components by
short name, or use diagnostic type names or function addresses as semantic
identities. Reuse existing import/dependency contracts; any new binding field
or author annotation needs separate owner approval. If a required registry
owner cannot be established, reject the reference rather than rebinding it.

Cross-component references require dependencies. Cross-registry references
also require the existing versioned, exact-digest imports. A same-named local
type must not capture an imported reference. Two distinct declarations must
not become one owner merely because their names and shapes match.

Keep the current CTSC outcome rules while resolving aliases:

| Resolved outer type | Recorded completion |
|---|---|
| Standard `Result<T, E>` | Successful `T`, or default `error` carrying the whole `E`. |
| Standard `Result<(), E>` | Unit success emits no result event. |
| Standard `Result<T, ()>` | Error has its name but no payload. |
| Standard `Option<T>` with non-unit payload | Successful `T`, or empty. |
| Standard `Option<()>` | Ordinary optional-unit value, retaining `Some`/`None`. |
| Reference/`Box` around an outcome | Ordinary value; do not promote the inner wrapper. |
| Ordinary named record/union | Ordinary named value, even if named `Result` or `Option`. |

Use the resolved payload to identify unit, not an alias's spelling. Preserve
nested tags and all current record, tuple, optional, and outcome projections.
This changes neither CTSC wire shapes nor the default error mapping.

Keep existing generic specialization restrictions. This ADR does not approve
channel-changing substitutions hidden by aliases or associated projections.
Activation must include rejection evidence for such unsupported forms; if a
generic form cannot be checked, keep it unsupported rather than silently
allowing a new completion channel.

### 4. Explicit compatibility boundary

| Existing author code | Proposed effect |
|---|---|
| Supported derived `SpecEvent` types | Derive supplies metadata without a second annotation. Preserve existing ignores, renames, and setup behavior. |
| Empty manual `SpecEvent` implementation | The implementation still compiles because of default `None`; discovery fails when that unavailable metadata is required. Add an explicit metadata implementation. |
| Manual `ToNativeValue` only | Serialization alone does not supply metadata. Implement `SpecEvent` where the typed discovery path requires it. |
| `dyn SpecEvent` | Remains usable for value projection; the static method is not dynamically dispatched. |
| Downstream `SpecEvent` implementations for references or `Box` | New supported blanket implementations can overlap. Review and document required downstream changes; do not promise full source compatibility. |

Treat this as a source-affecting SDK change with migration notes and coordinated
macro/runtime consumers, not a fully compatible marker-trait extension.
Preserve existing Cargo package-identity rules for generated runners.
Required metadata fails closed with operation/use-site diagnostics; do not
silently fall back to spelling or serialize a sample.

### 5. Narrow delivery-order exception

Permit the alias/shared-metadata foundation ahead of the lossless native
collection migration required by ADR-0019. This exception does not implement
`spec_collection`, change native sidecars, or move custom collection capture
ahead of lossless values.

Deliver passive metadata infrastructure first, then its production consumers
through bounded PRs. Before activation, identify the complete supported type
surface and preserve every supported operation/setup path. Do not enable a
partial global cutover and discover missing metadata from existing fixtures.
If activation cannot be split without two competing outcome classifiers,
keep that activation coordinated and review its size explicitly.

`Self` needs an additional enclosing-context decision before callbacks can be
enabled in contexts that cannot name the type today. This ADR does not select
an impl/module/file annotation or the experimental linker-symbol bridge.
Do not require an annotation on every file or silently change existing method
syntax to complete the rollout.

## Consequences

- One metadata source replaces discovery/capture disagreement. Supported
  aliases do not require a separate registration or wrapper-specific alias
  list.
- Named declaration graphs and ownership resolution require more work than
  copying the experiment's eager tree and local registry constant.
- Static metadata does not itself select a constructor, authorize field
  defaults, or implement replay. Existing setup rules and the separately
  approved enum-construction principle remain authoritative.
- New error-case mappings, collection authoring, general serializer-shape
  enforcement, C# changes, and comparison policy changes are outside this ADR.
- Until owner approval and production validation, this is a proposal, not
  shipped alias support or approval of a `Self` annotation.

## Validation

The experiment established feasibility for 29 real operation calls plus
focused negatives. It did not establish production readiness: its adapter
supports nested `Result` values only for unit/unit payloads, and its async
evidence covers construction-to-first-poll movement, not general resumption.
Full `just check` was interrupted, not passed.

Production acceptance requires ordinary tests through the actual CLI and
native boundaries, not only a descriptor-to-schema test adapter:

- Discover and capture imported one-argument `Result`, arbitrarily named
  aliases, aliased `Option`/unit, and ordinary types with misleading names.
- Verify current unit/error/optional rules, nested wrappers, supported
  references/`Box`, lazy exactly-once projection, ignored opaque inputs, and
  unsupported metadata diagnostics.
- Cover renamed and same-short-name declarations in different components,
  recursive named definitions, duplicate owners, real imported ownership,
  dependencies, and rejected missing/tampered imports.
- Cover existing record/union members, setup folding, borrowed values, concrete
  generic callbacks without stronger body bounds, hidden channel-change
  negatives, trait objects, and downstream coherence/compiler diagnostics.
- Preserve existing async first-poll, pending/resumption, thread-migration,
  parentage, and abandonment tests.
- Pass independent Registry, Trace, Linked, and bundle validation; keep the
  golden gate linked and strict. This adds no broad linked-validation walk
  during ordinary capture or replay.

Implementation seams: runtime type/link-time metadata, derive and operation/
setup macros, discovery normalization/ownership, native completion, and their
existing tests. Update affected docs, regenerate only approved golden changes,
and run `just check` for production slices. Each slice retains independent
adversarial review, final-head CI, and explicit human merge approval.

## Supersedes / superseded by

If accepted, refines the metadata API/compatibility choices and narrowly amends
delivery order in [ADR-0019](custom-collection-metadata.md). Its collection
annotation-error, construction, package-identity, and CTSC boundaries remain.
Update the [collection proposal](../contributor/custom-collection-contract.md)
to match if this exception is ratified; until then its accepted order remains
in force.

Preserves [typed generic replay](typed-async-client-replay.md),
[setup construction](constructor-and-property-setups.md), and the independent
[enum replay proposal](named-enum-replay-inputs.md). The future `Self` context
record is separate.
