# C# implementation conventions

## Project structure

```
src/<ComponentName>/
  <ComponentName>.csproj
  Implementation.cs
tests/<ComponentName>.Tests/
  <ComponentName>.Tests.csproj
  SpecTests.cs
```

## Annotations (when available)

C# uses attributes from the SpecGate annotation package.

| Annotation | Form | Example |
|------------|------|---------|
| `[SpecOperation("name", Kind = K)]` | attribute | `[SpecOperation("calc", Kind = OperationKind.StateMachine)]` |
| `[SpecSetup("op", Name = "n")]` | attribute | `[SpecSetup("calc", Name = "default")]` |
| `[SpecCheckpoint("op")]` | attribute | `[SpecCheckpoint("pipeline")]` on a method |
| `SpecCheckpoint.Capture("op", expr)` | static call | `SpecCheckpoint.Capture("pipeline", store.Count(item))` |
| `[SpecCapture("op")]` | attribute | `[SpecCapture("calc")]` on class or property |
| `[SpecMock("op", Name = "n")]` | attribute | `[SpecMock("calc", Name = "backend")]` |

Use `InternalsVisibleTo` for test access.

```csharp
// Class-level capture — all public properties captured
[SpecCapture("fetch")]
public record FetchResult(int StatusCode, string Body);

public static class Fetcher
{
    [SpecOperation("fetch", Kind = OperationKind.Stateless)]
    public static FetchResult Fetch(string url) { /* ... */ }
}
```

```csharp
// Property-level capture — only annotated properties captured
public class CircuitBreaker
{
    [SpecCapture("breaker")]
    public string State { get; private set; } = "closed";

    [SpecCapture("breaker")]
    public int FailureCount { get; private set; }

    [SpecOperation("breaker", Kind = OperationKind.StateMachine)]
    public void OnResult(bool success) { /* ... */ }

    [SpecMock("breaker", Name = "backend")]
    public bool CallBackend() { /* ... */ return true; }
}

public static class BreakerSetup
{
    [SpecSetup("breaker", Name = "default")]
    public static CircuitBreaker Create() => new CircuitBreaker();
}
```

```csharp
// Checkpoint — attribute form (every call recorded)
public class Pipeline
{
    [SpecCheckpoint("pipeline")]
    public bool Validate(string input) { /* ... */ return true; }
}

// Checkpoint — inline form (third-party types, specific expressions)
[SpecOperation("pipeline", Kind = OperationKind.Sequence)]
public Output Process(string input)
{
    var valid = SpecCheckpoint.Capture("pipeline", validator.Check(input));
    var parsed = SpecCheckpoint.Capture("pipeline", ThirdParty.Parse(input));
    return new Output(valid, parsed);
}
```

### Zero-cost in production

All annotation attributes use `[Conditional("SPECGATE")]`. The `SPECGATE`
define is set in Debug builds and absent in Release. `SpecCheckpoint.Capture()`
compiles to a pass-through when `SPECGATE` is not defined.

## Mapping spec types to C#

| Spec type | C# type |
|-----------|---------|
| `string` | `string` |
| `int` | `int` or `long` |
| `float` | `double` |
| `bool` | `bool` |
| `decimal` | `decimal` |
| `bytes` | `byte[]` |
| `List<T>` | `List<T>` or `IReadOnlyList<T>` |
| `Option<T>` | `T?` (nullable) |
| `Map<K, V>` | `Dictionary<K, V>` or `IReadOnlyDictionary<K, V>` |
| `oneof` | OneOf (see below) |
| `causes` | Base record + derived records (see below) |
| record (fields) | `record` or `class` with properties |

## Error handling

Spec `causes` map to base record + derived records. Adding a new cause is
non-breaking — no signatures change. 3rd party errors are wrapped via the
`InnerException` property. Use [OneOf](https://github.com/mcintyre321/OneOf)
(MIT) for `oneof` data types and outcome return types, but NOT for error causes.

**Dependencies:** `OneOf` + `OneOf.SourceGenerator`

### Spec

```yaml
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }

  GeometryError:
    causes:
      NegativeDimension: { field: string }
      TooManyVertices: { count: int }

outcome:
  oneof: [Ok, Error, Unrecoverable]
outputs:
  when Ok:
    area: float
  when Error:
    error: GeometryError
  when Unrecoverable:
    message: string
```

### C# mapping

```csharp
// oneof → OneOf with exhaustive matching
public record Circle(double Radius);
public record Rectangle(double Width, double Height);

[GenerateOneOf]
public partial class Shape : OneOfBase<Circle, Rectangle> { }

// causes → base record + derived (adding new causes is non-breaking)
public abstract record GeometryCause;
public record NegativeDimension(string Field) : GeometryCause;
public record TooManyVertices(int Count) : GeometryCause;

// Error wrapper holds cause + optional inner exception (for 3rd party errors)
public class GeometryError(GeometryCause cause, Exception? inner = null)
{
    public GeometryCause Cause => cause;
    public Exception? InnerException => inner;
}

// outcome oneof → OneOf return type (Unrecoverable → throw)
public OneOf<double, GeometryError> ComputeArea(Shape shape) { ... }
```

### Error vs Unrecoverable

| Spec outcome | C# pattern | Test assertion |
|---|---|---|
| `when Ok` | `result.AsT0` | `Assert.Equal(expected, result.AsT0)` |
| `when Error` | `result.AsT1` | `Assert.IsType<T>(error.Cause)` |
| `when Unrecoverable` | `throw` exception | `Assert.Throws<T>()` |

Error = caller can handle it. Unrecoverable = continuing would make things worse.

```csharp
[Fact]
public void CircleArea()
{
    var result = Geometry.ComputeArea(new Circle(5.0));
    Assert.Equal(78.54, result.AsT0, precision: 2);
}

[Fact]
public void NegativeRadius()
{
    var error = Geometry.ComputeArea(new Circle(-1)).AsT1;
    Assert.IsType<NegativeDimension>(error.Cause);
}

[Fact]
public void NullShapeAborts()
{
    Assert.Throws<ArgumentNullException>(
        () => Geometry.ComputeArea(null!));
}
```

## Async considerations

- `Task<T>` unwraps to `T` in spec types
- `Nullable<T>` and nullable reference types map to `Option<T>`
- Use `async Task` test methods with xUnit's async support
