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
| `oneof` | Abstract record with derived types, or discriminated union pattern |
| record (fields) | `record` or `class` with properties |

## Mapping spec oneof to C#

```yaml
# Spec
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }
```

```csharp
// Option A: abstract record hierarchy
public abstract record Shape;
public record Circle(double Radius) : Shape;
public record Rectangle(double Width, double Height) : Shape;

// Option B: tagged JSON with System.Text.Json
[JsonDerivedType(typeof(Circle), "Circle")]
[JsonDerivedType(typeof(Rectangle), "Rectangle")]
public abstract record Shape;
```

## Generating tests from spec cases

Each spec case maps to one `[Fact]` or `[Theory]`:

```csharp
[Fact]
public void CircleArea()
{
    // Arrange
    var shape = new Circle(5.0);

    // Act
    var area = Geometry.ComputeArea(shape);

    // Assert
    Assert.Equal(78.54, area, precision: 2);
}

[Fact]
public void DivideByZero()
{
    // Act
    var result = Calculator.Divide(10, 0);

    // Assert
    var error = Assert.IsType<CalcResult.Error>(result);
    Assert.Equal("division by zero", error.Message);
}
```

## Error handling

Use result types that match the spec outcome model:

```csharp
public abstract record CalcResult
{
    public record Ok(double Value) : CalcResult;
    public record Error(string Message) : CalcResult;
}
```

## Async considerations

- `Task<T>` unwraps to `T` in spec types
- `Nullable<T>` and nullable reference types map to `Option<T>`
- Use `async Task` test methods with xUnit's async support
