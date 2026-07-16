using System.Collections.Generic;
using SpecGate.Annotations;
using SpecGate.Runtime;

namespace SpecGateFixtures;

[SpecEvent]
public abstract class Shape
{
}

public sealed class Circle : Shape
{
    [SpecEvent("radius")]
    public double Radius { get; set; }
}

public sealed class Rectangle : Shape
{
    [SpecEvent("width")]
    public double Width { get; set; }

    [SpecEvent("height")]
    public double Height { get; set; }
}

public sealed class Point : Shape
{
}

public static class SumTypeFixtures
{
    [SpecOperation("classify")]
    public static Shape Classify([SpecInput("sides")] int sides) =>
        sides switch
        {
            1 => new Circle { Radius = 5.0 },
            4 => new Rectangle { Width = 3.0, Height = 4.0 },
            _ => new Point(),
        };

    [SpecOperation("find")]
    public static Option<int> Find([SpecInput("items")] List<int> items, [SpecInput("target")] int target)
    {
        int index = items.IndexOf(target);
        return index < 0 ? Option<int>.None() : Option<int>.Some(index);
    }

    [SpecOperation("try_divide")]
    public static Result<int,string> DivideResult([SpecInput("a")] int a, [SpecInput("b")] int b) =>
        b == 0 ? Result<int,string>.Err("division by zero") : Result<int,string>.Ok(a / b);
}
