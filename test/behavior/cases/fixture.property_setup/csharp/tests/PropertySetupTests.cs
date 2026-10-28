using SpecGate.Annotations;
using SpecGate.CtscFixtures.PropertySetups;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class PropertySetupTests
{
    [Fact]
    public void ConstructorAndPropertyInputsAreBehavioral()
    {
        _ = Assert.Throws<ArgumentException>(() => new Constructed("throw"));
        Assert.Equal("created", Operations.Constructor(new Constructed("created")));
        Assert.Equal(3, Operations.ZeroInputs(new Empty()));
        Assert.Equal("producer", Operations.ProducerPrecedence(Operations.Produce("producer")));
        var query = new Query
        {
            filter = "x",
            top = Option<uint>.Some(10),
            tags = ["a", "b"],
            allowed = ["b", "a"],
            weights = new SortedDictionary<string, Option<List<int>>>
            {
                ["some"] = Option<List<int>>.Some([2, 3]),
                ["none"] = Option<List<int>>.None(),
            },
        };
        Assert.Equal("x", Operations.ModuleRelative(query));
        Assert.Equal("x:10:a,b:a,b:5", Operations.AllFields(query));
        Assert.Equal(":0:::0", Operations.AllFields(new Query { top = Option<uint>.None() }));
        var subset = new Subset
        {
            Selected = 7,
            Cache = "not part of the contract",
        };
        Assert.Equal(9u, Operations.SubsetRenamed(subset, 2));
        Assert.Equal(10u, Operations.SubsetRenamed(subset, 2));
        Assert.Equal(9u, subset.Selected);
    }
}
