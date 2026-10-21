# SpecGate.Annotations

Attributes that declare C# operations, setups, inputs, structured types, and
expected exceptions for [SpecGate](https://github.com/schgoo/specgate).
SpecGate discovers them as a CTSC registry and captures ordinary tests that call
them as a reference trace.

The attributes are metadata only. Your projects do not change for capture:
`specgate capture` injects the `SpecGate.Weaver` IL weaver and the
`SpecGate.Runtime` reference into its own build of the sources project, which
records each annotated call. Woven capture needs .NET 8 or later; this package
targets `netstandard2.0`.

## Example

```csharp
using SpecGate.Annotations;

public static class Arithmetic
{
    [SpecOperation("add", Spec = "example.math")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
```

`Spec` is the component id. Tests that call `Arithmetic.Add` (xUnit, MSTest, or
NUnit) become capture scenarios.

## Attributes and types

| Item | Use |
| --- | --- |
| `[SpecOperation(name, Spec = component)]` | Marks a captured operation. |
| `[SpecSetup(name, Spec = component, Fills = parameter)]` | Marks a factory whose inputs become the named operation's inputs; `Fills` names a parameter instead of the receiver. |
| `[SpecInput(name)]` | Renames a parameter in the registry. |
| `[SpecEvent]`, `[SpecEvent(name)]` | Records a class, struct, or enum and its members as a structured type. |
| `[SpecException(types...)]` | Declares exceptions recorded as the `error` outcome; with no types, every exception. |
| `Option<T>` | Return type recorded as `result` or `empty`. |
| `Result<T, E>` | Return type recorded as `result` or `error`. |

Operations may return `Task`, `Task<T>`, `ValueTask`, or `ValueTask<T>`.

## Documentation

- [Instrument a C# assembly](https://github.com/schgoo/specgate/blob/main/docs/guides/instrument-csharp-assembly.md)
- [Capture a reference](https://github.com/schgoo/specgate/blob/main/docs/guides/capture-a-reference.md)
- [Async capture](https://github.com/schgoo/specgate/blob/main/docs/guides/async-capture.md)
- [Value mapping](https://github.com/schgoo/specgate/blob/main/docs/reference/value-mapping.md)
- [Limitations](https://github.com/schgoo/specgate/blob/main/docs/reference/limitations.md)
