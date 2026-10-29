# Value mapping reference

How discovery maps native Rust and C# types onto CTSC registry types. The
CTSC type system itself is normative and defined in
[`../ctsc/registry.md`](../ctsc/registry.md) §5; this page only describes what
SpecGate's producers emit today.

## Primitives

CTSC primitives are `unit`, `string`, `bool`, `i32`, `i64`, `u32`, `u64`,
`f32`, `f64`, and `bytes`.

| CTSC | Rust | C# |
| --- | --- | --- |
| `unit` | `()` or no return type | `void`, `Task`, `ValueTask` |
| `string` | `String`, `&str`, `ComponentId`, `OperationName`, `ImplementationLabel` | `string` |
| `bool` | `bool` | `bool` |
| `i32` | `i32` | `int` |
| `i64` | `i64` | `long` |
| `u32` | `u32` | `uint` (`System.UInt32`) |
| `u64` | `u64` | `ulong` (`System.UInt64`) |
| `f32` | `f32` | `float` |
| `f64` | `f64` | `double` |

Other native numeric types (Rust `i8`, `u8`, `i16`, `u16`, `i128`, `u128`,
`isize`, `usize`, `char`; C# `short`, `sbyte`, `byte`, `ushort`, and others)
are not CTSC primitives. Use one of the widths above at the annotated boundary.

### Unsigned integers

Discovery and capture use the declared boundary type, including types nested in
collections, optionals, and annotated members. The normative wire
representations are defined in [CTSC Trace](../ctsc/trace.md).

| Rust | C# | CTSC registry type | OTLP value |
|---|---|---|---|
| `u32` | `uint` (`System.UInt32`) | `u32` | `intValue`, from 0 through 4,294,967,295 |
| `u64` | `ulong` (`System.UInt64`) | `u64` | `stringValue`, canonical decimal from `"0"` through `"18446744073709551615"` |

The native runtime projects `u32`/`uint` as an integer and `u64`/`ulong` as an
unsigned value. The latter is encoded as decimal text even when it fits in a
signed integer; values above `long.MaxValue` are preserved without narrowing.
Decimal text has no sign, separators, whitespace, or leading zeroes except `"0"`.

These mappings apply to operation inputs and results and recurse through
supported value shapes. C# nullable unsigned integers use the same optional
projection as other nullable values: nested values carry `Some` or `None`, while
an outer nullable return selects the result or empty outcome. A C# `object`
holding a boxed `uint` or `ulong` uses the corresponding native value family.

The [`fixture.unsigned`](../../test/behavior/cases/fixture.unsigned/) case captures
zero, signed maxima, the first values above those maxima, and unsigned maxima
through ordinary annotated operations in both languages. Its golden matrix row
requires identical registries, linked native bundles, and successful replay of
the C# reference on Rust.

## Composite types

| CTSC kind | Rust | C# |
| --- | --- | --- |
| `list` | `Vec<T>`, `&[T]` | `T[]`, `List<T>` |
| `set` | `HashSet<T>`, `BTreeSet<T>` | `HashSet<T>`, `SortedSet<T>` |
| `map` | `HashMap<K, V>`, `BTreeMap<K, V>` | `Dictionary<K, V>`, `SortedDictionary<K, V>` |
| `optional` | `Option<T>` (inputs, fields) | `SpecGate.Annotations.Option<T>`, `T?` (nullable value or reference type) |
| `record` | `struct` with `#[derive(SpecEvent)]` | non-abstract class or struct with `[SpecEvent]`; members marked `[SpecEvent]` become fields |
| `tagged_union` | `enum` with `#[derive(SpecEvent)]` | `enum` with `[SpecEvent]`; abstract class with `[SpecEvent]` (its concrete subclasses become variants) |

Rust references (`&T`, `&mut T`) map to `T`. Only the final path segment of a
Rust type is used, so `std::collections::BTreeMap` and `BTreeMap` are the same.

Rust enum variants map to a `tagged_union` variant with no payload (unit
variant), a `tuple` payload (tuple variant), or a `record` payload (struct
variant). C# enum members map to variants without payloads, in declaration
order.

A named type must be annotated (`SpecEvent`) to appear in the registry. Other
named types are emitted by name only and are not supported at an annotated
boundary.

## Operation outcomes

The return type maps to the CTSC outcome channels
([`../ctsc/registry.md`](../ctsc/registry.md)):

| Return type | Registry outcome |
| --- | --- |
| `()` / `void` / `Task` | No `result`. |
| `T` | `result: T`. |
| `Option<T>` (Rust or `SpecGate.Annotations.Option<T>`) | `result: T` and `empty: true`. `None` completes through the empty channel. |
| `Option<()>` | `result: optional<unit>`; no empty channel. |
| `Result<T, E>` (Rust or `SpecGate.Annotations.Result<T, E>`) | `result: T` (omitted when `T` is unit) and one error named `error` with type `E`. |
| C# method with `[SpecException]` | As `Result<T, string>`: one error named `error` of type `string`. |
| C# `Task<T>` / `ValueTask<T>` | As `T`; the operation is async. |

## Recorded values

Capture records each value at the operation boundary, while the call is on the
stack. Rust and C# produce the same shapes, so their traces can be compared:

| Value | Recorded as |
| --- | --- |
| Record | Map from field name to value. |
| Tagged-union variant | Single-entry map from variant name to its payload; a payload-free variant (including a C# enum member) has the empty map as payload. |
| Optional | `Some` with the value, or `None`. |
| Nested `Result` | `Ok` or `Err` with the value. |
| Outermost `Option` / `Result` of a return value | Unwrapped onto the `result`, `empty`, or `error` channel, as in the table above. |

C# capture also rejects, with an error naming the value:

- `null` where the declared type is not nullable. Declare the parameter, member,
  or return value nullable to record absence.
- An enum value that is not one of the enum's declared members (for example an
  undeclared numeric value or a `[Flags]` combination).
- A map whose keys are not strings.
- A string that contains an unpaired UTF-16 surrogate.
- A value declared as `object` whose runtime type is not a primitive or a
  `[SpecEvent]`-annotated type. Collections are refused because a list and a
  set cannot be told apart once the declared type is erased.

## Candidate replay values

Rust replay accepts lossless primitives and recursively supported lists, sets,
string-keyed maps, and optionals as operation and setup construction inputs.
Classes and structs opted in as property setups (`#[spec_event(setup)]`,
`[SpecEvent(Setup = true)]`) are deconstructed into their selected fields,
recursing through fields that are themselves opted-in records, including
arguments to explicit setup producers. Leaves become flat inputs under their
semantic names in depth-first declaration order; each leaf must be a primitive,
collection, or optional. Duplicate leaf names and cyclic property-setup types
are rejected. Replay rebuilds the nested Rust value with `Default` for omitted
fields.
Structured results and declared errors, including lists of annotated records,
are supported. The candidate's reachable named CTSC declarations must match the
reference, including declarations reached only by nested observed operations.
Replay validates recorded candidate outcomes against the reference registry
before publishing the trace.

Other record inputs, including property-setup fields holding records that are
not opted in and records inside collections or optionals, and
explicit reference-to-candidate shape mappings remain unsupported; record
inputs fail with the `structured-value` replay category
([constructor and property setups](../decisions/constructor-and-property-setups.md)).
A tuple payload and a record payload are different declarations; replay does
not silently translate between them.

Generic operations publish concrete signatures from explicit Rust `types` or
C# `TypeArguments` annotations. See
[`../status.md`](../status.md#candidate-replay-and-explicit-generic-operations)
for specialization syntax and completion-channel restrictions.

## Names

- Rust field names and C# member names become CTSC field names. Rust fields can
  be renamed with `#[spec_event(name = "wire_name")]`; C# members with
  `[SpecEvent("wire_name")]`.
- C# parameter names can be set with `[SpecInput("name")]`. Variant and type
  names can be set the same way with `[SpecEvent("name")]`.
- To compare a Rust and a C# implementation, both must publish the same names
  and kinds. Check with `specgate discover` on each side.

## Source of truth

- Rust: `rust/crates/specgate-discovery/src/discovery/types.rs` and
  `src/discovery/normalize/schema.rs`.
- C#: `csharp/SpecGate.Discovery/SemanticTypes.cs` and `ComponentDocument.cs`.
- Registry conversion: `rust/crates/specgate-ctsc/src/registry/normalized.rs`.
