# Native value mapping

Discovery and capture use the declared boundary type, including types nested in
collections, optionals, and annotated members. The normative types and wire
representations are defined in [CTSC Registry](../ctsc/registry.md) and
[CTSC Trace](../ctsc/trace.md).

## Unsigned integers

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
