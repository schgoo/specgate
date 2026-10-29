# Constructor and property setups

> **Status:** accepted
> **Owner:** @schgoo (user-ratified)
> **Date:** 2026-10-23

## Decision

Setup producers are the semantic mapping boundary for native input objects.
C# constructors may declare `[SpecSetup]`: their parameters are construction
inputs and their declaring type is the produced value.

Property setups require an explicit type-level opt-in in each language.
Rust uses `#[derive(SpecEvent)]` with `#[spec_event(setup)]`; C# uses
`[SpecEvent(Setup = true)]`.
Participating fields become flat operation inputs in declaration order. With no
field annotations, all fields participate; otherwise only annotated fields
participate. Existing semantic field names resolve naming differences.
Duplicate folded input names are rejected, never prefixed or overwritten.
Explicit producers take precedence for the parameters they fill.

Capture decomposes property-setup arguments at operation entry, after object
initializers and any other caller-side mutation. Discovery and capture publish
the same folded surface. Rust replay reconstructs its own value with a struct
literal, decoding selected fields from the reference inputs. Participating
fields must be accessible to the generated invocation.

The property-setup opt-in is an author promise that omitted fields may safely
use `Default::default()`. Omission from recording alone is not evidence of
irrelevance and does not authorize reconstruction. A subset requires the Rust
type to implement `Default`; failure is a compile-time error. Zero-input
property setups may reconstruct an entirely default value. Types with
construction invariants must use an explicit producer instead.

Replay also accepts recursively supported lists, sets, string-keyed maps, and
optionals as operation and construction inputs. Primitive range, floating-point,
optional-tag, and collection semantics remain unchanged.

### Recursive decomposition (2026-10-28)

Property decomposition follows nested, explicitly opted-in record fields
recursively, including property-setup arguments to explicit setup producers.
Leaves appear in depth-first declaration order under their semantic field names.
Names remain flat: collisions anywhere in the folded operation are errors.
Resolve each nested field in its declaring type's native module, not the
operation's module. Cyclic property-setup declarations are rejected rather than
expanded indefinitely.

Capture snapshots the same leaves at operation or producer entry. Rust replay
reconstructs the nested record shape before invoking the operation or producer.
Empty nested property setups and omitted-field defaults retain the rules above.
An explicit producer still takes precedence over property decomposition of the
operation parameter it fills.

This recursive step does not descend into optional, list, set, or map elements.
Containers of supported primitive values remain supported; records inside
containers and undeclared record reconstruction remain outside this slice.
C# rejects null in a non-nullable nested property-setup member as `NullValue`,
as it rejects null in any non-nullable declared type. A nullable member (`T?`)
maps to `optional`, the same as Rust `Option<T>`, and is not decomposed.

## Boundaries

This supersedes the prohibition on defaulting omitted fields in
[`typed-async-client-replay.md`](typed-async-client-replay.md) only for explicit
property setups. `SpecEvent` projection alone never opts a type into input
reconstruction. General record reconstruction remains unsupported. No CTSC
document shape, CLI argument, comparison policy, runtime identity rule, or
golden-matrix meaning changes.

## Evidence

Native CTSC fixtures must cover constructor provenance, operation-entry property
projection, all-fields and subset reconstruction, renamed fields, folded-name
collisions, missing `Default`, and C#-reference-to-Rust replay of collection and
optional fields. Generated bundles must validate and paired registries must
agree.
