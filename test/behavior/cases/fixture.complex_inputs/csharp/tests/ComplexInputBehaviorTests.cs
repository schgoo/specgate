using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.ComplexInputs;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class ComplexInputBehaviorTests
{
    [Fact]
    public void StructuredInputsAndOutputsUseRealValues()
    {
        List<EnumMemberInput> members =
        [
            new() { Name = "Red", Value = "1" },
            new() { Name = "Blue", Value = "2" },
        ];
        Assert.Equal("Color", ComplexInputs.CreateEnumType("Color", members));
        Assert.Equal(2, ComplexInputs.MemberCount(members));
        Assert.Equal("Red", ComplexInputs.FirstMember(members));

        Point sum =
            ComplexInputs.SumPoints([new() { X = 1, Y = 2 }, new() { X = 3, Y = 4 }]);
        Assert.Equal(4, sum.X);
        Assert.Equal(6, sum.Y);
        Assert.Equal(
            "demo",
            ComplexInputs.DescribeConfig(
                new AppConfig { Name = "demo", MaxRetries = 3, Verbose = true }));
        Assert.Equal(
            12,
            ComplexInputs.AreaOfShape(new Rectangle { Width = 3, Height = 4 }));
        Assert.IsType<Rectangle>(ComplexInputs.Classify(4));
        Assert.IsType<ShapePoint>(ComplexInputs.Classify(1));
    }

    [Fact]
    public void CollectionAndOptionalSurfacesPreserveSemantics()
    {
        List<Point> line = ComplexInputs.GetPointsOnLine(3);
        Assert.Equal([0, 1, 2], line.Select(point => point.X));

        SortedDictionary<string, int> table = new()
        {
            ["alpha"] = 7,
            ["beta"] = 9,
        };
        Assert.Equal(9, ComplexInputs.Lookup(table, "beta"));
        Assert.Equal("alpha", ComplexInputs.InvertMap(table)["7"]);
        Assert.Equal("Hello, Ada!", ComplexInputs.GreetOptional(Option<string>.Some("Ada")));
        Assert.Equal("Hello, stranger!", ComplexInputs.GreetOptional(Option<string>.None()));

        Option<Point> found =
            ComplexInputs.FindPoint([new() { X = 2, Y = 8 }, new() { X = 4, Y = 16 }], 4);
        Assert.True(found.HasValue);
        Assert.Equal(16, found.Value!.Y);
        Assert.False(ComplexInputs.FindPoint([], 4).HasValue);
        Assert.IsType<Circle>(ComplexInputs.FindShape(1).Value);
        Assert.IsType<ShapePoint>(ComplexInputs.FindShape(0).Value);
        Assert.False(ComplexInputs.FindShape(3).HasValue);
    }

    [Fact]
    public void NestedRecordsRoundTripThroughOperations()
    {
        Person person = ComplexInputs.CreatePerson("Ada", 36, "1 Main", "London");
        Assert.Equal("Ada, age 36", ComplexInputs.DescribePerson(person));
        Assert.Equal("1 Main", person.Address.Street);
        Assert.Equal("London", person.Address.City);
    }
}
