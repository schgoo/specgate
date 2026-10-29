# Instrument a C# assembly

## Goal

Annotate a C# project so `specgate` can discover its component surface and
capture its tests as reference behavior.

## Prerequisites

- The CLI with its C# tools beside it ([`install.md`](install.md)).
- The .NET 10 runtime for the C# discovery tool (`SpecGate.Discovery` targets
  `net10.0`; building it from source needs the .NET 10 SDK).
- An SDK that builds your projects. The project that holds your operations must
  target .NET 8 or later for capture (`SpecGate.Runtime` targets `net8.0` and
  `net10.0`).
- A sources project that references the `SpecGate.Annotations` package.
- A separate tests project that references the sources project and uses exactly
  one of xUnit (`xunit` or `xunit.v3`), MSTest, or NUnit.

You do not edit either project for capture. `specgate capture` injects the IL
weaver and the `SpecGate.Runtime` reference into its own build of the sources
project.

## Steps

### 1. Annotate operations

```csharp
using SpecGate.Annotations;

namespace Example;

public static class Arithmetic
{
    [SpecOperation("add", Spec = "fixture.cli_capture.add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
```

[it] [smoke] (from `rust/crates/specgate-cli/tests/fixtures/csharp-capture/sources/Operations.cs`)

- `Spec` is the component id and is required for capture.
- Repeat `[SpecOperation]` only for distinct explicit generic specializations.
- `[SpecInput("name")]` sets an input name; without it the parameter name is
  used.

### 2. Reuse the existing tests

Capture runs the tests that already call your annotated API, unchanged. Start
there. Add a test only for behavior those tests do not cover, and report which
tests you added
([choose the boundary](README.md#step-0-choose-the-boundary-before-instrumenting)).
A test must call the requested API itself, not a new wrapper that selects
hard-coded data by name.

```csharp
using Xunit;

public sealed class CaptureTests
{
    [Fact]
    public void AddsTwoNumbers()
    {
        Assert.Equal(5, Arithmetic.Add(2, 3));
    }
}
```

Each listed test becomes one scenario named by its display name. Theories and
parameterized tests produce one scenario per case.

### 3. Annotate structured types

Mark each recorded type and member with `[SpecEvent]`. An optional argument
renames it.

```csharp
[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }
}
```

| C# declaration | CTSC type |
| --- | --- |
| Class or struct with `[SpecEvent]` | Record of its public `[SpecEvent]` properties and fields declared on that type |
| Abstract class with `[SpecEvent]` | Tagged union whose variants are its concrete `[SpecEvent]` subclasses |
| Enum with `[SpecEvent]` | Tagged union of its members, in declaration order, without payloads; `[SpecEvent("…")]` on a member renames it |

See [`../reference/value-mapping.md`](../reference/value-mapping.md) for every
supported type and the values capture rejects.

### 4. Declare outcomes

| Return or annotation | Recorded outcome |
| --- | --- |
| `T` | `result` |
| `void`, `Task`, `ValueTask` | No result |
| `SpecGate.Annotations.Option<T>` | `result` when present, `empty` when absent |
| `SpecGate.Annotations.Result<T, E>` | `result` when `Ok`, `error` when `Err` |
| `[SpecException(typeof(X))]` and a matching exception is thrown | `error` with the exception message as its value |
| `[SpecException]` with no types | Every exception is the `error` outcome |
| Any other exception | An unexpected target fault |

### 5. Annotate setups for stateful operations

```csharp
[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.setup")]
    public static Counter Make([SpecInput("initial")] int initial) => new() { Count = initial };

    [SpecOperation("increment", Spec = "fixture.setup")]
    public void Increment() => Count++;
}
```

The setup's inputs become the operation's inputs. Set `Fills = "param"` when a
setup produces a parameter rather than the receiver. C# setup inputs can drive
annotated Rust candidate producers during replay; C# candidate replay remains
unsupported ([limitations](../reference/limitations.md)).

`[SpecSetup]` may also annotate a constructor: its declaring type is the
produced value, and its parameters are the construction inputs.

For object-initializer parameters, use `[SpecEvent(Setup = true)]` on the type.
Without member annotations, all public instance fields and readable properties
declared on the type participate; otherwise only annotated members participate.
Capture reads their final values at operation entry and folds their semantic
names into flat inputs. A member whose type is itself `Setup = true` is folded
recursively, including in setup-producer arguments; records inside collections
are not. Rename members with `[SpecEvent("...")]` to resolve collisions; names
are never automatically prefixed. Duplicate names and types that reach
themselves are rejected as `AmbiguousDeclaration`, and a null property-setup
value as `NullValue`. Explicit setup producers take precedence over property
folding for the parameters they fill. Rust candidates opt in separately and
promise that any omitted fields may safely use `Default`.

### 6. Async operations

Operations returning `Task`, `Task<T>`, `ValueTask`, or `ValueTask<T>` are
supported; see [`async-capture.md`](async-capture.md).

### 7. Explicit generic specializations

Give each specialization a distinct operation name and concrete type arguments:
`[SpecOperation("find_items", Spec = "example.client", TypeArguments = new[] { typeof(Item) })]`.
Repeat the annotation for other specializations. Generic constraints still
apply; calling an undeclared specialization during capture fails. Open generic
declaring types and generic setups remain unsupported.

## Expected output

```sh
specgate capture --binding specgate.yaml --component fixture.cli_capture.add --impl csharp --out capture
```

prints `Complete(component=fixture.cli_capture.add, scenarios=…, …)` and exits 0.
[smoke] [it]

## Artifacts

None from instrumentation. Capture writes a bundle; see
[`capture-a-reference.md`](capture-a-reference.md).

## Failure modes

The weaver fails the capture build with a message naming the method:

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `Async capture supports operations returning Task, Task<T>, ValueTask, or ValueTask<T>` | An `async void` operation, an async setup, or an `async` method returning another awaitable type. | Return `Task` / `ValueTask` (or a `<T>` form); make setups synchronous. |
| `Iterator capture is not supported` / `Stream capture is not supported` | An iterator or async iterator. | Return a materialized collection. |
| `requires one concrete TypeArguments entry per generic method parameter` | Missing or incomplete specialization. | Supply concrete `TypeArguments` for each generic method parameter. |
| `is a generic method` / `is declared on generic type` | Generic setup or generic declaring type. | Use a nongeneric setup on a nongeneric type. |
| `must take parameters by value` | A `ref`, `out`, `in`, or pointer parameter. | Take parameters by value. |
| `annotation without a Spec component` | `[SpecOperation]` without `Spec`. | Add `Spec = "<component>"`. |
| `the capture weaver wove no method of` | Discovery found operations but the woven build did not contain them. | Check that `sources` points at the project holding the annotated methods. |

More in [`troubleshoot-capture.md`](troubleshoot-capture.md).

## Verify

```sh
specgate validate bundle capture
```

prints `valid: bundle capture` and exits 0. [smoke]

## Source of truth

- [`csharp/SpecGate.Annotations/SpecGateAnnotations.cs`](../../csharp/SpecGate.Annotations/SpecGateAnnotations.cs)
- `csharp/SpecGate.Weaver/AnnotatedMethod.cs`, `OperationDeclaration.cs` — weave rejections.
- `csharp/SpecGate.Runtime/OperationMetadata.cs`, `ValueProjector.cs` — what is recorded.
- [`decisions/csharp-capture-instrumentation.md`](../decisions/csharp-capture-instrumentation.md)
- [`decisions/csharp-capture-cli.md`](../decisions/csharp-capture-cli.md)
- [`decisions/csharp-async-capture.md`](../decisions/csharp-async-capture.md)
