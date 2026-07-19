using SpecGate.Annotations;
using SpecGate.Runtime;

namespace SpecGateFixtures.ComplexInputs;

/// <summary>A single enum member supplied as structured list input.</summary>
public class EnumMemberInput
{
    /// <summary>The member's name.</summary>
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>The member's value, carried as a string.</summary>
    [SpecEvent("value")]
    public string Value { get; set; } = string.Empty;
}

/// <summary>A two-dimensional point used as a struct input and output.</summary>
public class Point
{
    /// <summary>The x coordinate.</summary>
    [SpecEvent("x")]
    public int X { get; set; }

    /// <summary>The y coordinate.</summary>
    [SpecEvent("y")]
    public int Y { get; set; }
}

/// <summary>An application configuration struct deserialized from a nested mapping input.</summary>
public class AppConfig
{
    /// <summary>The application name.</summary>
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>The maximum retry count.</summary>
    [SpecEvent("max_retries")]
    public int MaxRetries { get; set; }

    /// <summary>Whether verbose logging is enabled.</summary>
    [SpecEvent("verbose")]
    public bool Verbose { get; set; }
}

/// <summary>
/// A shape sum type used as both an enum-variant input and output. Variants
/// serialize as tagged maps keyed by variant name (<c>Circle</c>,
/// <c>Rectangle</c>, <c>Point</c>).
/// </summary>
[SpecEvent]
public abstract class Shape
{
}

/// <summary>The circle variant, carrying an integer radius.</summary>
public sealed class Circle : Shape
{
    /// <summary>The circle's radius.</summary>
    [SpecEvent("radius")]
    public int Radius { get; set; }
}

/// <summary>The rectangle variant, carrying width and height.</summary>
public sealed class Rectangle : Shape
{
    /// <summary>The rectangle's width.</summary>
    [SpecEvent("width")]
    public int Width { get; set; }

    /// <summary>The rectangle's height.</summary>
    [SpecEvent("height")]
    public int Height { get; set; }
}

/// <summary>
/// The payloadless point variant. Named <c>ShapePoint</c> to avoid clashing
/// with the <see cref="Point"/> struct, but tagged <c>Point</c> so it
/// serializes as <c>{Point: {}}</c>, matching the reference trace.
/// </summary>
[SpecEvent("Point")]
public sealed class ShapePoint : Shape
{
}

/// <summary>A postal address, used as a nested struct within <see cref="Person"/>.</summary>
public class Address
{
    /// <summary>The street line.</summary>
    [SpecEvent("street")]
    public string Street { get; set; } = string.Empty;

    /// <summary>The city.</summary>
    [SpecEvent("city")]
    public string City { get; set; } = string.Empty;
}

/// <summary>A person with a nested <see cref="Address"/>, exercising nested struct input/output.</summary>
public class Person
{
    /// <summary>The person's name.</summary>
    [SpecEvent("name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>The person's age.</summary>
    [SpecEvent("age")]
    public int Age { get; set; }

    /// <summary>The person's address (nested struct).</summary>
    [SpecEvent("address")]
    public Address Address { get; set; } = new();
}

/// <summary>
/// Operations exercising complex structured inputs and outputs — lists of
/// structs, nested structs, enum variants, maps, and optionals — materialized
/// from the spec case inputs and serialized back into canonical trace values.
/// </summary>
public static class ComplexInputOps
{
    /// <summary>Records member statistics for a named enum built from a list of members.</summary>
    /// <param name="name">The enum type name (spec input <c>name</c>).</param>
    /// <param name="members">The member definitions (spec input <c>members</c>).</param>
    /// <returns>The enum <paramref name="name"/> unchanged.</returns>
    [SpecOperation("create_enum_type")]
    public static string CreateEnumType([SpecInput("name")] string name, [SpecInput("members")] List<EnumMemberInput> members)
    {
        SpecGateRuntime.EmitEvent("member_count", members.Count);
        string first = members.Count > 0 ? members[0].Name : string.Empty;
        SpecGateRuntime.EmitEvent("first_member", first);
        return name;
    }

    /// <summary>Sums a list of points component-wise.</summary>
    /// <param name="points">The points to sum (spec input <c>points</c>).</param>
    /// <returns>A <see cref="Point"/> whose coordinates are the component sums.</returns>
    [SpecOperation("sum_points")]
    public static Point SumPoints([SpecInput("points")] List<Point> points)
    {
        int x = 0, y = 0;
        foreach (var p in points)
        {
            x += p.X;
            y += p.Y;
        }

        return new Point { X = x, Y = y };
    }

    /// <summary>Returns the name of a configuration struct input.</summary>
    /// <param name="config">The configuration to describe (spec input <c>config</c>).</param>
    /// <returns>The config's <see cref="AppConfig.Name"/>.</returns>
    [SpecOperation("describe_config")]
    public static string DescribeConfig([SpecInput("config")] AppConfig config) => config.Name;

    /// <summary>Computes the area of a shape variant input.</summary>
    /// <param name="shape">The shape (spec input <c>shape</c>).</param>
    /// <returns>The integer area: circle area for <see cref="Circle"/>, width*height for <see cref="Rectangle"/>, 0 for a point.</returns>
    [SpecOperation("area_of_shape")]
    public static int AreaOfShape([SpecInput("shape")] Shape shape) => shape switch
    {
        Circle c => (int)(Math.PI * c.Radius * c.Radius),
        Rectangle r => r.Width * r.Height,
        _ => 0,
    };

    /// <summary>Classifies a side count into a <see cref="Shape"/> variant.</summary>
    /// <param name="sides">The side count (spec input <c>sides</c>).</param>
    /// <returns>A <see cref="Rectangle"/> for 4, a point for 1, otherwise a <see cref="Circle"/>.</returns>
    [SpecOperation("classify")]
    public static Shape Classify([SpecInput("sides")] int sides) => sides switch
    {
        4 => new Rectangle { Width = 3, Height = 4 },
        1 => new ShapePoint(),
        _ => new Circle { Radius = 5 },
    };

    /// <summary>Returns a diagonal line of points.</summary>
    /// <param name="count">The number of points to generate (spec input <c>count</c>).</param>
    /// <returns>A list of <paramref name="count"/> points where each has equal x and y.</returns>
    [SpecOperation("get_points_on_line")]
    public static List<Point> GetPointsOnLine([SpecInput("count")] int count)
    {
        var points = new List<Point>();
        for (int i = 0; i < count; i++)
        {
            points.Add(new Point { X = i, Y = i });
        }

        return points;
    }

    /// <summary>Looks up a key in a string-to-int map input.</summary>
    /// <param name="table">The map to search (spec input <c>table</c>).</param>
    /// <param name="key">The key to look up (spec input <c>key</c>).</param>
    /// <returns>The mapped value, or 0 when the key is absent.</returns>
    [SpecOperation("lookup")]
    public static int Lookup([SpecInput("table")] Dictionary<string, int> table, [SpecInput("key")] string key) =>
        table.TryGetValue(key, out int value) ? value : 0;

    /// <summary>Inverts a string-to-int map into an int-to-string map (values become keys).</summary>
    /// <param name="table">The map to invert (spec input <c>table</c>).</param>
    /// <returns>A map keyed by the stringified original values.</returns>
    [SpecOperation("invert_map")]
    public static Dictionary<string, string> InvertMap([SpecInput("table")] Dictionary<string, int> table)
    {
        var inverted = new Dictionary<string, string>();
        foreach (var entry in table)
        {
            inverted[entry.Value.ToString(System.Globalization.CultureInfo.InvariantCulture)] = entry.Key;
        }

        return inverted;
    }

    /// <summary>Greets an optional name, falling back to a generic greeting when absent.</summary>
    /// <param name="name">The optional name (spec input <c>name</c>).</param>
    /// <returns><c>"Hello, {name}!"</c> when present, otherwise <c>"Hello, stranger!"</c>.</returns>
    [SpecOperation("greet_optional")]
    public static string GreetOptional([SpecInput("name")] Option<string> name) =>
        name.HasValue ? $"Hello, {name.Value}!" : "Hello, stranger!";

    /// <summary>Finds the first point with a matching x coordinate.</summary>
    /// <param name="points">The points to search (spec input <c>points</c>).</param>
    /// <param name="targetX">The x coordinate to match (spec input <c>target_x</c>).</param>
    /// <returns><c>Some</c> point when found, otherwise <c>None</c>.</returns>
    [SpecOperation("find_point")]
    public static Option<Point> FindPoint([SpecInput("points")] List<Point> points, [SpecInput("target_x")] int targetX)
    {
        foreach (var p in points)
        {
            if (p.X == targetX)
            {
                return Option<Point>.Some(p);
            }
        }

        return Option<Point>.None();
    }

    /// <summary>Optionally produces a shape for a given side count.</summary>
    /// <param name="sides">The side count (spec input <c>sides</c>).</param>
    /// <returns><c>Some(Circle)</c> for 1, <c>Some(Point)</c> for 0, otherwise <c>None</c>.</returns>
    [SpecOperation("find_shape")]
    public static Option<Shape> FindShape([SpecInput("sides")] int sides) => sides switch
    {
        1 => Option<Shape>.Some(new Circle { Radius = 5 }),
        0 => Option<Shape>.Some(new ShapePoint()),
        _ => Option<Shape>.None(),
    };

    /// <summary>Describes a person built from a nested struct input.</summary>
    /// <param name="person">The person to describe (spec input <c>person</c>).</param>
    /// <returns>The string <c>"{name}, age {age}"</c>.</returns>
    [SpecOperation("describe_person")]
    public static string DescribePerson([SpecInput("person")] Person person) => $"{person.Name}, age {person.Age}";

    /// <summary>Builds a person (with a nested address) from flat inputs.</summary>
    /// <param name="name">The person's name (spec input <c>name</c>).</param>
    /// <param name="age">The person's age (spec input <c>age</c>).</param>
    /// <param name="street">The address street (spec input <c>street</c>).</param>
    /// <param name="city">The address city (spec input <c>city</c>).</param>
    /// <returns>A <see cref="Person"/> with the given fields and a nested <see cref="Address"/>.</returns>
    [SpecOperation("create_person")]
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
