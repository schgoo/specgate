using System.Globalization;
using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.ComplexInputs;

[SpecEvent("EnumMemberInput")]
public sealed class EnumMemberInput
{
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("value")]
    public string Value { get; set; } = string.Empty;
}

[SpecEvent("Point")]
public sealed class Point
{
    [SpecEvent("x")]
    public int X { get; set; }

    [SpecEvent("y")]
    public int Y { get; set; }
}

[SpecEvent("AppConfig")]
public sealed class AppConfig
{
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("max_retries")]
    public int MaxRetries { get; set; }

    [SpecEvent("verbose")]
    public bool Verbose { get; set; }
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
public sealed class ShapePoint : Shape;

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

    [SpecEvent("age")]
    public int Age { get; set; }

    [SpecEvent("address")]
    public Address Address { get; set; } = new();
}

public static class ComplexInputs
{
    [SpecOperation("create_enum_type", Spec = "fixture.complex_inputs")]
    public static string CreateEnumType(
        [SpecInput("name")] string name,
        [SpecInput("members")] List<EnumMemberInput> members)
    {
        _ = MemberCount(members);
        _ = FirstMember(members);
        return name;
    }

    [SpecOperation("member_count", Spec = "fixture.complex_inputs")]
    public static int MemberCount([SpecInput("members")] List<EnumMemberInput> members) =>
        members.Count;

    [SpecOperation("first_member", Spec = "fixture.complex_inputs")]
    public static string FirstMember([SpecInput("members")] List<EnumMemberInput> members) =>
        members.Count == 0 ? string.Empty : members[0].Name;

    [SpecOperation("sum_points", Spec = "fixture.complex_inputs")]
    public static Point SumPoints([SpecInput("points")] List<Point> points) =>
        new()
        {
            X = points.Sum(point => point.X),
            Y = points.Sum(point => point.Y),
        };

    [SpecOperation("describe_config", Spec = "fixture.complex_inputs")]
    public static string DescribeConfig([SpecInput("config")] AppConfig config) => config.Name;

    [SpecOperation("area_of_shape", Spec = "fixture.complex_inputs")]
    public static int AreaOfShape([SpecInput("shape")] Shape shape) =>
        shape switch
        {
            Circle circle => (int)(Math.PI * circle.Radius * circle.Radius),
            Rectangle rectangle => rectangle.Width * rectangle.Height,
            _ => 0,
        };

    [SpecOperation("classify", Spec = "fixture.complex_inputs")]
    public static Shape Classify([SpecInput("sides")] int sides) =>
        sides switch
        {
            4 => new Rectangle { Width = 3, Height = 4 },
            1 => new ShapePoint(),
            _ => new Circle { Radius = 5 },
        };

    [SpecOperation("get_points_on_line", Spec = "fixture.complex_inputs")]
    public static List<Point> GetPointsOnLine([SpecInput("count")] int count)
    {
        List<Point> points = [];
        for (int value = 0; value < count; value++)
        {
            points.Add(new Point { X = value, Y = value });
        }

        return points;
    }

    [SpecOperation("lookup", Spec = "fixture.complex_inputs")]
    public static int Lookup(
        [SpecInput("table")] SortedDictionary<string, int> table,
        [SpecInput("key")] string key) =>
        table.GetValueOrDefault(key);

    [SpecOperation("invert_map", Spec = "fixture.complex_inputs")]
    public static SortedDictionary<string, string> InvertMap(
        [SpecInput("table")] SortedDictionary<string, int> table)
    {
        SortedDictionary<string, string> inverted = [];
        foreach (KeyValuePair<string, int> entry in table)
        {
            inverted[entry.Value.ToString(CultureInfo.InvariantCulture)] = entry.Key;
        }

        return inverted;
    }

    [SpecOperation("greet_optional", Spec = "fixture.complex_inputs")]
    public static string GreetOptional([SpecInput("name")] Option<string> name) =>
        name.HasValue ? $"Hello, {name.Value}!" : "Hello, stranger!";

    [SpecOperation("find_point", Spec = "fixture.complex_inputs")]
    public static Option<Point> FindPoint(
        [SpecInput("points")] List<Point> points,
        [SpecInput("target_x")] int targetX)
    {
        Point? point = points.Find(point => point.X == targetX);
        return point is null ? Option<Point>.None() : Option<Point>.Some(point);
    }

    [SpecOperation("find_shape", Spec = "fixture.complex_inputs")]
    public static Option<Shape> FindShape([SpecInput("sides")] int sides) =>
        sides switch
        {
            1 => Option<Shape>.Some(new Circle { Radius = 5 }),
            0 => Option<Shape>.Some(new ShapePoint()),
            _ => Option<Shape>.None(),
        };

    [SpecOperation("describe_person", Spec = "fixture.complex_inputs")]
    public static string DescribePerson([SpecInput("person")] Person person) =>
        $"{person.Name}, age {person.Age}";

    [SpecOperation("create_person", Spec = "fixture.complex_inputs")]
    public static Person CreatePerson(
        [SpecInput("name")] string name,
        [SpecInput("age")] int age,
        [SpecInput("street")] string street,
        [SpecInput("city")] string city) =>
        new()
        {
            Name = name,
            Age = age,
            Address = new Address { Street = street, City = city },
        };
}
