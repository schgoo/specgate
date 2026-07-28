using SpecGate.Annotations;
using SpecGate.Runtime;
namespace SpecGateFixtures.Conformance.SumTypes;

/// <summary>
/// Abstract base of a sum type (discriminated union). Concrete subclasses
/// (<see cref="Circle"/>, <see cref="Rectangle"/>, <see cref="Point"/>)
/// serialize as tagged maps keyed by their type name.
/// </summary>
[SpecEvent]
public abstract class Shape
{
}

/// <summary>A circle variant carrying its radius.</summary>
public sealed class Circle : Shape
{
    /// <summary>The circle's radius.</summary>
    [SpecEvent("radius")]
    public double Radius { get; set; }
}

/// <summary>A rectangle variant carrying its width and height.</summary>
public sealed class Rectangle : Shape
{
    /// <summary>The rectangle's width.</summary>
    [SpecEvent("width")]
    public double Width { get; set; }

    /// <summary>The rectangle's height.</summary>
    [SpecEvent("height")]
    public double Height { get; set; }
}

/// <summary>A payloadless point variant (unit-like shape).</summary>
public sealed class Point : Shape
{
}

/// <summary>
/// Fixtures exercising sum-type return values: an enum-like
/// <see cref="Shape"/> hierarchy, plus <see cref="Option{T}"/> and
/// <see cref="Result{T, E}"/>, all serialized as canonical tagged maps.
/// </summary>
public static class SumTypeFixtures
{
    /// <summary>Classifies a side count into a <see cref="Shape"/> variant.</summary>
    /// <param name="sides">The number of sides (spec input <c>sides</c>).</param>
    /// <returns>A <see cref="Circle"/> for 1, a <see cref="Rectangle"/> for 4, otherwise a <see cref="Point"/>.</returns>
    [SpecOperation("classify", Spec = "fixture.enum_event")]
    public static Shape Classify([SpecInput("sides")] int sides) =>
        sides switch
        {
            1 => new Circle { Radius = 5.0 },
            4 => new Rectangle { Width = 3.0, Height = 4.0 },
            _ => new Point(),
        };

    /// <summary>Finds the index of <paramref name="target"/> within <paramref name="items"/>.</summary>
    /// <param name="items">The list to search (spec input <c>items</c>).</param>
    /// <param name="target">The value to locate (spec input <c>target</c>).</param>
    /// <returns>The index as a nullable <c>int?</c>, or <c>null</c> when absent (spec <c>Option&lt;i32&gt;</c>).</returns>
    [SpecOperation("find", Spec = "fixture.option_some")]
    [SpecOperation("find", Spec = "fixture.option_none")]
    public static int? Find([SpecInput("items")] List<int> items, [SpecInput("target")] int target)
    {
        int index = items.IndexOf(target);
        return index < 0 ? null : index;
    }

    /// <summary>Divides <paramref name="a"/> by <paramref name="b"/>, realizing a spec <c>Result&lt;i32, string&gt;</c>.</summary>
    /// <param name="a">The dividend (spec input <c>a</c>).</param>
    /// <param name="b">The divisor (spec input <c>b</c>).</param>
    /// <returns>The quotient (Ok arm); throws <see cref="DivideByZeroException"/> for the Err arm when <paramref name="b"/> is zero.</returns>
    [SpecOperation("try_divide", Spec = "fixture.result_ok")]
    [SpecOperation("try_divide", Spec = "fixture.result_err")]
    [SpecException(typeof(DivideByZeroException))]
    public static int DivideResult([SpecInput("a")] int a, [SpecInput("b")] int b)
    {
        if (b == 0)
        {
            throw new DivideByZeroException("division by zero");
        }

        return a / b;
    }
}
