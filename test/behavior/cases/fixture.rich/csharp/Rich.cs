using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Registry.Focused.Rich;

[SpecEvent("Address")]
public sealed class Address
{
    [SpecEvent("street")]
    public string Street { get; set; } = string.Empty;

    [SpecEvent("city")]
    public string City { get; set; } = string.Empty;
}

[SpecEvent("Person")]
public sealed class Person
{
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("address")]
    public Address Address { get; set; } = new();

    [SpecEvent("tags")]
    public List<string> Tags { get; set; } = [];

    [SpecEvent("scores")]
    public SortedDictionary<string, int> Scores { get; set; } = [];

    [SpecEvent("aliases")]
    public SortedSet<string> Aliases { get; set; } = [];

    [SpecEvent("nickname")]
    public Option<string> Nickname { get; set; } = Option<string>.None();
}

[SpecEvent("Shape")]
public abstract class Shape;

[SpecEvent("Circle")]
public sealed class Circle : Shape
{
    [SpecEvent("radius")]
    public int Radius { get; set; }
}

[SpecEvent("Rectangle")]
public sealed class Rectangle : Shape
{
    [SpecEvent("width")]
    public int Width { get; set; }

    [SpecEvent("height")]
    public int Height { get; set; }
}

[SpecEvent("Point")]
public sealed class Point : Shape;

public static class Rich
{
    [SpecOperation("describe", Spec = "fixture.rich")]
    public static Option<string> Describe(
        [SpecInput("person")] Person person,
        [SpecInput("fallback")] Option<Shape> fallback)
    {
        _ = TagCount(person.Tags);
        return fallback.HasValue
            ? Option<string>.Some(person.Name)
            : Option<string>.None();
    }

    [SpecOperation("tag_count", Spec = "fixture.rich")]
    public static int TagCount([SpecInput("tags")] List<string> tags) => tags.Count;
}
