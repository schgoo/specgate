# C# implementation conventions

> C# backend support is **planned**. The current fixture suite is
> Rust-only. This page documents the intended annotation surface so it
> stays aligned with the simplified Event/Run trace model. Treat the
> code samples as design intent, not as a working API.

## Project structure

```
src/<ComponentName>/<ComponentName>.csproj
tests/<ComponentName>.Tests/<ComponentName>.Tests.csproj
```

Use `InternalsVisibleTo` to give the test project access to annotated
internals.

## Annotations (planned)

The C# attributes mirror the five Rust macros one-for-one:

| Rust | C# | Placed on |
|------|-----|-----------|
| `#[spec_operation("name")]` | `[SpecOperation("name")]` | Method |
| `#[spec_setup("name")]` | `[SpecSetup("name")]` | Static factory method (no `this`) |
| `#[spec_event]` on a field | `[SpecEvent]` on a property | Property with a setter |
| `spec_event!("name", expr)` | `SpecEvent.Record("name", expr)` | Inline expression |
| `#[spec_mock("name")]` | `[SpecMock("name")]` | Call site or method |

No `Kind` parameter. The shape of the operation is expressed through
what the spec's `expected:` list contains, exactly as in Rust.

```csharp
using SpecGate.Annotations;

public static class Math
{
    [SpecOperation("add")]
    public static int Add(int a, int b) => a + b;
}

public class Counter
{
    [SpecEvent]
    public int Count { get; private set; }

    [SpecOperation("increment")]
    public void Increment() => Count += 1;
}

public static class CounterFactory
{
    [SpecSetup("make_counter")]
    public static Counter MakeCounter() => new Counter();
}
```

## Return value conventions

Same as Rust — see [`rust.md`](rust.md). The trace name convention is
language-agnostic (`<operation>.result`, `<operation>.outcome`,
`<operation>.error`).
