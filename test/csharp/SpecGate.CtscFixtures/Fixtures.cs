using SpecGate.Annotations;

namespace SpecGate.CtscFixtures;

/// <summary>Stateless discovery counterpart to the Rust replay fixture.</summary>
public static class Stateless
{
    /// <summary>Adds two integers.</summary>
    [SpecOperation("add", Spec = "fixture.stateless_add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}

/// <summary>A nested address record.</summary>
[SpecEvent]
public sealed class Address
{
    /// <summary>Street name.</summary>
    [SpecEvent("street")]
    public string Street { get; set; } = string.Empty;

    /// <summary>City name.</summary>
    [SpecEvent("city")]
    public string City { get; set; } = string.Empty;
}

/// <summary>A rich semantic record.</summary>
[SpecEvent]
public sealed class Person
{
    /// <summary>Person name.</summary>
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>Nested address.</summary>
    [SpecEvent("address")]
    public Address Address { get; set; } = new();

    /// <summary>Ordered tags.</summary>
    [SpecEvent("tags")]
    public List<string> Tags { get; set; } = [];

    /// <summary>Named scores.</summary>
    [SpecEvent("scores")]
    public SortedDictionary<string, int> Scores { get; set; } = [];

    /// <summary>Unique aliases.</summary>
    [SpecEvent("aliases")]
    public SortedSet<string> Aliases { get; set; } = [];

    /// <summary>Optional nickname.</summary>
    [SpecEvent("nickname")]
    public Option<string> Nickname { get; set; } = Option<string>.None();
}

/// <summary>Tagged shape union.</summary>
[SpecEvent]
public abstract class Shape;

/// <summary>Circle variant.</summary>
[SpecEvent("Circle")]
public sealed class Circle : Shape
{
    /// <summary>Circle radius.</summary>
    [SpecEvent("radius")]
    public int Radius { get; set; }
}

/// <summary>Rectangle variant.</summary>
[SpecEvent("Rectangle")]
public sealed class Rectangle : Shape
{
    /// <summary>Rectangle width.</summary>
    [SpecEvent("width")]
    public int Width { get; set; }

    /// <summary>Rectangle height.</summary>
    [SpecEvent("height")]
    public int Height { get; set; }
}

/// <summary>Point variant.</summary>
[SpecEvent("Point")]
public sealed class Point : Shape;

/// <summary>Rich discovery operations.</summary>
public static class Rich
{
    /// <summary>Returns a description when a fallback shape exists.</summary>
    [SpecOperation("describe", Spec = "fixture.rich")]
    public static Option<string> Describe(
        [SpecInput("person")] Person person,
        [SpecInput("fallback")] Option<Shape> fallback) =>
        fallback.HasValue
            ? Option<string>.Some(person.Name)
            : Option<string>.None();
}

/// <summary>Setup-folding fixture.</summary>
[SpecEvent]
public sealed class Counter
{
    /// <summary>Current counter value.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Constructs a counter for increment.</summary>
    [SpecSetup("increment", Spec = "fixture.setup")]
    public static Counter Make([SpecInput("initial")] int initial) => new() { Count = initial };

    /// <summary>Increments the counter.</summary>
    [SpecOperation("increment", Spec = "fixture.setup")]
    public void Increment() => Count++;
}

/// <summary>Fallible unit and unscoped-setup discovery fixture.</summary>
[SpecEvent]
public sealed class UnitCounter
{
    /// <summary>Current counter value.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Unscoped setup metadata applies to the selected component.</summary>
    [SpecSetup("advance")]
    public static UnitCounter Make([SpecInput("initial")] int initial) => new() { Count = initial };

    /// <summary>Advances the counter.</summary>
    [SpecOperation("advance", Spec = "fixture.fallible_unit")]
    public void Advance() => Count++;
}

/// <summary>Fallible operations whose successful channel is unit.</summary>
public static class FallibleUnit
{
    /// <summary>Completes without a value or throws a declared error.</summary>
    [SpecOperation("fallible_void", Spec = "fixture.fallible_unit")]
    [SpecException(typeof(InvalidOperationException))]
    public static void FallibleVoid([SpecInput("fail")] bool fail)
    {
        if (fail)
        {
            throw new InvalidOperationException("failed");
        }
    }

    /// <summary>Asynchronously completes without a value or throws a declared error.</summary>
    [SpecOperation("fallible_task", Spec = "fixture.fallible_unit")]
    [SpecException(typeof(InvalidOperationException))]
    public static Task FallibleTask([SpecInput("fail")] bool fail) =>
        fail ? Task.FromException(new InvalidOperationException("failed")) : Task.CompletedTask;
}
