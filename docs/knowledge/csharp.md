# C# implementation conventions

> C# is a first-class conformance target. Fixtures under
> `test/csharp/SpecGateFixtures` are dual-bound against their Rust
> counterparts and must emit byte-identical traces. This page documents
> the C# annotation surface and how idiomatic C# realizes the
> language-agnostic spec contract.

## Project structure

```
src/<ComponentName>/<ComponentName>.csproj
tests/<ComponentName>.Tests/<ComponentName>.Tests.csproj
```

Use `InternalsVisibleTo` to give the test project access to annotated
internals.

## Annotations (planned)

The C# attributes mirror the Rust macros, plus `[SpecException]` for the
`Result` error channel (which Rust expresses through the return type):

| Rust | C# | Placed on |
|------|-----|-----------|
| `#[spec_operation("name")]` | `[SpecOperation("name")]` | Method |
| `#[spec_setup("name")]` | `[SpecSetup("name")]` | Static factory method (no `this`) |
| `#[spec_event]` on a field | `[SpecEvent]` on a property | Property with a setter |
| `spec_trace!("name", expr)` | `SpecEvent.Record("name", expr)` | Inline statement |
| `#[spec_mock("name")]` | `[SpecMock("name")]` | Call site or method |
| `Result<T, E>` return type | `[SpecException(...)]` on the method | Method (see below) |

`SpecEvent.Record` is the only fixture-facing trace primitive (the
inline analog of the `[SpecEvent]` attribute). The runner-internal
`SpecGateRuntime` emit methods are hidden from IntelliSense
(`[EditorBrowsable(Never)]`) — fixtures never call them directly.

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

The trace names are language-agnostic. A value result is emitted as
`$result`; variant returns (`Result`, `Option`) emit `$result` as a
**tagged-variant map**; an uncaught fault is `$fault`:

| Spec `$result` type | Trace on success | Trace on failure |
|---------------------|------------------|------------------|
| `T` (plain value) | `$result: value` | `$fault: <msg>` (any thrown exception) |
| `Option<T>` | `$result: { Some: value }` / `{ None: {} }` | `$fault: <msg>` |
| `Result<T, E>` | `$result: { Ok: value }` | `$result: { Err: <msg> }` (declared) / `$fault: <msg>` (undeclared) |

Conformance is verified by **trace byte-identity**, not by a separate
type-checking gate. The spec's declared type is the contract; idiomatic
C# realizes it as follows.

### Option → nullable `T?`

An `Option<T>` operation returns a nullable `T?` (value **or**
reference type — the project sets `<Nullable>enable</Nullable>`):

```csharp
[SpecOperation("find")]
public static int? Find(List<int> items, int target)
{
    int i = items.IndexOf(target);
    return i < 0 ? null : i;      // null -> { None: {} }, value -> { Some: value }
}
```

The `?` in the return type is the signal — the generated runner wraps
the result in `Some`/`None`. (Runtime reflection can only see nullable
*value* types; reference nullability like `Shape?` is erased, so the
shape is derived from the declared return type, not the runtime object.)
Optional **inputs** are likewise idiomatic nullable parameters (`T?`); a
`None` input materializes as `null`.

### Result → `throw`, declared via `[SpecException]`

A `Result<T, E>` operation returns the bare `Ok` type `T` and **throws**
for the `Err` arm. `[SpecException]` marks the operation as fallible so
the runner wraps the outcome:

```csharp
// Precise: only the listed exception types are the Err arm;
// any other throw is an undeclared fault ($fault).
[SpecOperation("try_divide")]
[SpecException(typeof(DivideByZeroException))]
public static int TryDivide(int a, int b)
{
    if (b == 0) throw new DivideByZeroException("division by zero");
    return a / b;   // -> { Ok: 5 };  throw -> { Err: "division by zero" }
}

// Catch-all: every thrown exception is the Err arm (no $fault path).
[SpecOperation("parse")]
[SpecException]
public static int Parse(string s) => int.Parse(s);
```

- normal return → `$result: { Ok: value }`
- a **declared** exception (or any, for the no-arg form) →
  `$result: { Err: ex.Message }`
- any **other** exception → `$fault: ex.Message`

The `Err` value is the exception's `Message`, so it must match the
spec's expected `Err` string byte-for-byte.

### Panic / unrecoverable

A panic is **undeclared by definition**: an operation with no
`[SpecException]` that throws surfaces the exception as `$fault`. This is
the exact analog of a Rust `panic!` and needs no annotation — the
operation is typed as a plain return, and a case simply asserts
`$fault: <msg>` where it expects one.

### Other shapes

| Return shape | Trace | Spec asserts |
|--------------|-------|--------------|
| `void` / no return | no `$result` event | rely on `[SpecEvent]` property events |

`[SpecEvent]` property naming follows Rust — see [`rust.md`](rust.md).
