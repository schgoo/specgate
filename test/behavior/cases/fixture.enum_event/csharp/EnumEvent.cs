using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

[SpecEvent("Shape")]
public abstract class Shape;

[SpecEvent("Circle")]
public sealed class Circle : Shape
{
    [SpecEvent("radius")]
    public double Radius { get; set; }
}

[SpecEvent("Rectangle")]
public sealed class Rectangle : Shape
{
    [SpecEvent("width")]
    public double Width { get; set; }

    [SpecEvent("height")]
    public double Height { get; set; }
}

[SpecEvent("Tag")]
public sealed class Tag : Shape
{
    [SpecEvent("value")]
    public string Value { get; set; } = string.Empty;
}

[SpecEvent("Point")]
public sealed class Point : Shape;

public static class EnumEvent
{
    [SpecOperation("classify", Spec = "fixture.enum_event")]
    public static Shape Classify([SpecInput("sides")] int sides) =>
        sides switch
        {
            0 => new Point(),
            1 => new Circle { Radius = 5.0 },
            3 => new Tag { Value = "triangle" },
            4 => new Rectangle { Width = 3.0, Height = 4.0 },
            _ => new Point(),
        };
}
