# C# implementation conventions

## Project structure

```
src/<ComponentName>/<ComponentName>.csproj
tests/<ComponentName>.Tests/<ComponentName>.Tests.csproj
```

## Annotations

C# uses attributes from the SpecGate annotation package.

```csharp
// Stateless operation
[SpecOperation("area", Kind = OperationKind.Stateless)]
public static double ComputeArea(Shape shape) { ... }

// Setup — constructs inputs for tests (no this)
[SpecSetup("area", Name = "circle")]
public static Shape MakeCircle(double radius)
    => new Circle(radius);

// Capture on class — all public properties captured
[SpecCapture("area")]
public record AreaResult(double Area, double Perimeter);

// Capture on individual properties
public class Canvas
{
    [SpecCapture("canvas")]
    public double TotalArea { get; private set; }

    [SpecOperation("canvas", Kind = OperationKind.StateMachine)]
    public void AddShape(Shape shape) { ... }

    [SpecMock("canvas", Name = "renderer")]
    public byte[] Render() { ... }

    [SpecCheckpoint("canvas")]
    public (double, double) CurrentBounds() { ... }
}

// Inline checkpoint
[SpecOperation("pipeline", Kind = OperationKind.Sequence)]
public Output Process(string input)
{
    var validated = SpecCheckpoint.Capture("pipeline", validator.Check(input));
    return new Output(validated);
}
```

Use `InternalsVisibleTo` for test access.

See `annotations.md` for zero-cost production behavior.

## Mapping spec types to C#

| Spec type | C# type |
|-----------|---------|
| `string` | `string` |
| `int` | `int` or `long` |
| `float` | `double` |
| `bool` | `bool` |
| `List<T>` | `List<T>` |
| `Option<T>` | `T?` (nullable) |
| `Map<K, V>` | `Dictionary<K, V>` |
| `oneof` | OneOf (see below) |
| `causes` | Base record + derived records |
| record | `record` or `class` |

## Spec to C# mapping example

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
      UnsupportedShape: { name: string }

outcome:
  oneof: [Ok, Error]
outputs:
  when Ok:
    area: float
  when Error:
    error: GeometryError
```

### C#

```csharp
// oneof → OneOf
public record Circle(double Radius);
public record Rectangle(double Width, double Height);

[GenerateOneOf]
public partial class Shape : OneOfBase<Circle, Rectangle> { }

// causes → base record + derived
public abstract record GeometryCause;
public record NegativeDimension(string Field) : GeometryCause;
public record UnsupportedShape(string Name) : GeometryCause;

public class GeometryError(GeometryCause cause, Exception? inner = null)
{
    public GeometryCause Cause => cause;
    public Exception? InnerException => inner;
}

// outcome → OneOf return type
public OneOf<double, GeometryError> ComputeArea(Shape shape) { ... }
```

## Generated tests

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
```

## Error handling

| Spec outcome | C# pattern | Test assertion |
|---|---|---|
| `when Ok` | `result.AsT0` | `Assert.Equal(expected, result.AsT0)` |
| `when Error` | `result.AsT1` | `Assert.IsType<T>(error.Cause)` |
| `when Unrecoverable` | `throw` | `Assert.Throws<T>()` |

**Dependencies:** `OneOf` + `OneOf.SourceGenerator` (MIT)
