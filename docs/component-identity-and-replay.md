# Component identity, scenario identity, and the replay unit

> **Status:** exploratory. This note records open questions and the evidence
> behind them. It is not a decision record and ratifies nothing.
>
> **Date:** 2026-09-25
>
> **Related:** capture profiles (#43), stable operation identity (#10)

## Why this note exists

Capture profiles proposed a selection mechanism before the model it selects
against is settled. This note separates what is fixed by construction from what
is genuinely open, so that later decision records can be scoped one at a time.

## Fixed by construction

**A component is the semantic mapping.** Differential testing compares a
reference and a candidate that claim the same component surface. If a
component's meaning changes between implementations, the comparison is invalid
by construction, not merely unsupported. The exact component match in
`link_operation` (`rust/crates/specgate-cli/src/replay.rs:440`) is therefore the
contract working as intended, not a limitation to relax.

Renaming a component without changing its meaning is a separate, narrow
ergonomic concern. Stable declared identity already exists to make renames
unnecessary.

**Artifacts do not expose package organization.** Registry IDs derive from the
component set, the capture manifest carries the binding target label rather than
a package name, and trace resources carry target, tool, and registry identity
only. Packages are a build concern, not an artifact concern. This property
should be preserved.

## Open question 1 - scenario identity

SpecGate gives operations stable, declared, language-neutral identity through
`#[spec_operation("snake_case_name")]`. Scenarios have no equivalent. A scenario
name is whatever the test framework called the function: the libtest name, or
`binary::test_name` for non-library targets.

`ctsc.strict/0.1.0` pairs scenarios **by name**
(`rust/crates/specgate-ctsc/src/comparison.rs:187-194`).

Consequences:

- Replay hides the problem. The candidate trace is generated from the reference
  bundle, so scenario names are inherited and pairing always succeeds.
- Independent paired-native capture does not. Two implementations captured
  separately pair only if their test function names coincide, which they will
  not across languages or across a rewrite.

CTSC anticipated this: scenario matching is an explicit policy dimension
(`docs/ctsc/comparison.md:65`, "paired by name, position, selection, or another
key"). SpecGate implements only the fixed strict policy, so the extension point
exists and is unused.

The open question is whether scenario identity should be declared and
language-neutral, mirroring operation identity, rather than inherited from the
test framework.

## Open question 2 - what replay consumes

Replay currently anchors to one component: `bundle.component_id` selects the
candidate components to discover (`replay.rs:208`), resolves a single
`ResolvedCandidate` holding one schema and one package (`replay.rs:316`), and
rejects bundles whose selection spans components
(`rust/crates/specgate-ctsc/src/lib.rs:588-601`).

That anchoring is largely vestigial. The trace already determines the stimuli;
the component ID is used only to choose candidate schemas and to match
operations. Deriving the component set from the trace's own spans would make
multi-component replay fall out within a single package.

This reframes selection: **selection is a capture-side concern**. By the time
replay runs, the trace already is the selection. Replay does not need to know
whether a profile produced the bundle.

The open question is whether the replay unit is the trace, the component, or the
selection, and whether replay should stop asking for a component at all.

## Open question 3 - artifact granularity

A working preference is **one registry and trace per top-level component**.

Component-mode capture already produces exactly this: operations whose parent is
the scenario span and whose component matches are retained, together with their
subtrees kept verbatim, including nested operations from other components.

A profile selecting operations across components breaks the invariant, because
the resulting bundle has no single top-level component. This is a real tension,
not an implementation gap.

The open question is whether one-top-level-component-per-bundle should become a
stated invariant, and if so what that implies for multi-component profiles.

## Open question 4 - registry identity versus registry content

Section 3.1 removed the selected component's privileged position from component
*ordering*. It remains privileged in *identity*: `registryId` is
`urn:ctsc:registry:<selected-component>`, so the identifier names a root that
the document itself no longer distinguishes.

Two selections that close over the same component set therefore emit a
byte-identical `components` array under different registry IDs, and so different
document digests for the same graph. `registryId` is producer intent, not a
function of content.

A registry document MAY import other documents, and imports carry `sha256:`
digests (`docs/ctsc/registry.md`, section 2). Because those digest strings are
part of the root document's bytes, a root digest transitively pins its intended
closure. That is a Merkle DAG, and it is the property a bare symbolic reference
would not provide.

The resolved registry set is nevertheless the root document plus imported
documents **loaded for validation**. Validation is relative to what a caller
happened to load, and nothing in the artifacts records which closure was
resolved, so a valid result does not distinguish fully resolved from partially
resolved. For differential testing this matters more than for a policy system:
if a reference capture and a candidate replay resolve different registry sets,
the comparison runs over different surfaces and neither artifact records it.

This is latent today. Registries carry their closure inline, imports are unused,
and every corpus document is single-component, so root digest and closure digest
coincide. It becomes live the moment bundles are fragmented per component, which
is what open question 3 proposes.

The open question is whether registry identity should be derived from content,
retained as producer intent, or separated into both, and whether validation
should report its resolved set as a first-class outcome.

## Open question 5 - process boundaries

SpecGate records operation parentage from a thread-local stack: `parent_span_id`
is the innermost active operation
(`rust/crates/specgate-runtime/src/lib.rs:857`). There is no span context
propagation anywhere in the workspace, and CTSC already states the consequence:
different target executions do not exchange span context
(`docs/ctsc/trace.md`, section 5).

A service-to-service call therefore produces two unrelated traces in two
processes. The caller's client-side function and the callee's handler are both
ordinary annotated operations, but nothing records that one invoked the other.

Nested cross-component capture is the in-process analogue and already works:
component A's operation calling component B's operation is retained with
parentage intact, and both components are declared in a union registry. The
missing pieces are transport and aggregation, not shape.

Three further constraints apply independently:

- Native capture rejects async operations before polling, and a remote call in
  Rust is almost always async.
- `specgate capture` accepts Rust targets only.
- Replay re-invokes an operation in process. Replaying a caller requires the
  callee either replayed in lockstep or substituted from its recorded
  responses. The second turns recorded downstream results into stimuli, which is
  a different replay model than the one CTSC describes. Consumer-driven contract
  testing is the established prior art.

The open question is whether SpecGate's unit of observation stops at the process
boundary by definition, or whether distributed capture is a later capability
that the current artifacts should leave room for.

## Open question 6 - declared external identity

A component ID is a free-form declared string
(`rust/crates/specgate-annotations-macros/src/lib.rs:670`), unique across a
resolved registry set (`docs/ctsc/registry.md:54`), and deliberately
version-free so that a reference and a candidate pair on it.

Nothing relates that string to whatever another system calls the same unit of
software: a service entry in an authorization policy, a node in a service
graph, a package coordinate, a deployment identity. An external consumer of
SpecGate artifacts must therefore recover the correspondence heuristically, by
name similarity or by matching operation surfaces against routes. Heuristic
correlation is measurable but never exact, and it fails silently in both
directions.

Declaration is the alternative: a component states the identities it is known
by elsewhere, and correlation becomes a lookup. This adds no correlation logic
to SpecGate; it carries assertions their owners already hold. Section 3 already
admits optional namespaced extensions on a component, and section 11 requires
extensions to use a namespace outside `conformance.*`, so the format may
accommodate this without a shape change.

Two properties would have to hold. External identifiers must be inert metadata
rather than identity: if they participate in operation linking, the exact
component match that makes a comparison valid by construction weakens into
fuzzy matching. And they must not reintroduce package organization into
artifacts, which this note states should be preserved. A deployment or policy
identity is not a package name, but the line is thin.

Declared identity is viable at scale. Substrate's OMAP authorization policies
are authored by humans through a portal and rendered by a tool rather than
derived from code, so the authoritative dimension of at least one large service
graph is already a declaration of intent rather than an inference.

The open question is whether components should carry declared external
identifiers, and if so whether those identifiers are inert metadata or a
resolution mechanism.

## Open question 7 - boundary contracts versus in-process differential

Question 5 asks whether distributed capture is a later capability. This
question is prior to it: whether a SpecGate artifact describes one component's
behavior, or an interaction between two components.

Today it describes behavior. A capture drives operations in one process and
records what happened inside it, and replay re-invokes those operations in
process. Nested cross-component calls are retained verbatim with parentage
intact, but they are observed detail within a single execution rather than a
contract between two independently deliverable units.

The alternative model records the interaction at a boundary and replays each
side independently against the recorded counterpart: the callee against
recorded requests, the caller against recorded responses. Consumer-driven
contract testing is the established prior art. Question 5 identifies this as
necessary for service-to-service replay, but it is not only about distribution.
It also changes what an in-process bundle means, because a nested foreign
operation is exactly such a boundary.

The models differ in what a reported difference means. A whole-component
differential reports that observable behavior changed. A boundary contract
reports that a specific promise between two named parties was broken, which is
the form an external service graph can attach to an edge. Edge existence is
already available from ordinary distributed tracing; a typed, value-level
contract on the edge is not.

An adjacent stance follows from this framing. Deriving component and operation
identity from unmodified source, by crawling routes, clients, and call sites
the way a service-graph builder does, would make a boundary a matter of
inference. An annotation is what marks a boundary as carrying a promise its
owner intends to keep, which is an editorial judgement rather than a fact
recoverable from code. Recording that reasoning here avoids relitigating it per
feature.

The open question is whether a boundary contract is a second artifact kind
alongside the component bundle, a reinterpretation of the existing one, or out
of scope.

## Constraints any answer must respect

- A trace is bound to one root registry through required resource attributes
  (`docs/ctsc/trace.md:455-458`), resolved with its imports during Linked
  validation (`docs/ctsc/registry.md`, section 9). A registry document may
  declare many components and import others (section 2).
- One trace corresponds to one target execution. Different target executions do
  not exchange span context (`docs/ctsc/trace.md`, section 5), so a single trace
  cannot span separately built and separately run artifacts.
- Generated runners must resolve exactly one `specgate-runtime` package
  identity, so a candidate spanning several packages is constrained by the
  runtime package-identity rules rather than by CTSC.

## Where profiles sit

Capture profiles were implemented and then deliberately not merged. The work is
preserved unmerged rather than shipped, because it adds a selection mechanism —
a YAML format, a JSON schema, a CLI flag, a capture-manifest migration, and a
second filtering semantics — ahead of the model it selects against.

What the exercise established is worth keeping. Selecting a subset of one
component's operations and promoting a nested operation into a directly driven
stimulus are both useful, and a profile spanning components can capture and
validate but cannot replay.

Profiles should not be revived until questions 1 through 5 are resolved.
Questions 6 and 7 are broader model questions, not prerequisites for that
work.
