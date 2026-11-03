using SpecGate.CtscFixtures.Conformance.Collections;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class CollectionShapeBehaviorTests
{
    [Fact]
    public async Task CollectionContractsRetainOrderMultiplicityAndPriority()
    {
        IEnumerable<CollectionItem> items = await CollectionShapes.ItemsAsync();
        Assert.Equal([2, 1, 2], items.Select(item => item.Value));
        Assert.Equal([1, 2, 2], CollectionShapes.OrderedItems());
        Assert.Equal(["b", "a"], CollectionShapes.OrderedMap().Keys);
        PriorityQueue<string, int> queue = CollectionShapes.Priorities();
        Assert.Equal(2, queue.Count);
        Assert.Equal("first", queue.Peek());
        Assert.Contains(("later", 7), queue.UnorderedItems);
    }

    [Fact]
    public void LookupsRetainKeysAndDuplicateValues()
    {
        ILookup<string, int> lookup = CollectionShapes.Lookup();
        Assert.Equal(2, lookup.Count);
        Assert.Equal([2, 2], lookup["b"]);
        Assert.Equal([1], lookup["a"]);
        Assert.Empty(lookup["missing"]);
        ILookup<int, string> numbered = CollectionShapes.NumberLookup();
        Assert.Equal(2, numbered.Count);
        Assert.Equal(["two", "two"], numbered[2]);
        Assert.Equal(["one"], numbered[1]);
        ILookup<string?, int> optional = CollectionShapes.OptionalLookup();
        Assert.Equal(2, optional.Count);
        Assert.Equal([2, 2], optional[null]);
        Assert.Equal([1], optional["a"]);
    }

    [Fact]
    public void GroupingsRetainTheirKeyAndMultiplicity()
    {
        IGrouping<string, int> grouping = CollectionShapes.Grouping();
        Assert.Equal("group", grouping.Key);
        Assert.Equal([2, 1, 2], grouping);
        IGrouping<string?, int> optional = CollectionShapes.OptionalGrouping();
        Assert.Null(optional.Key);
        Assert.Equal([2, 1, 2], optional);
    }

    [Fact]
    public void CollectionInputsRetainTheirDeclaredSemantics()
    {
        IEnumerable<int> bag = [2, 1, 2];
        ISet<int> set = new HashSet<int> { 2, 1 };
        OrderedDictionary<string, int> ordered = new() { ["b"] = 2, ["a"] = 1 };
        Dictionary<int, string> numbered = new() { [2] = "two", [1] = "one" };
        Dictionary<string, IEnumerable<(int, int)>> nested = new()
        {
            ["group"] = [(2, 3), (1, 4), (2, 3)],
        };

        Assert.Equal(5, CollectionShapes.BagInput(bag));
        Assert.Equal(3, CollectionShapes.SetInput(set));
        Assert.Equal("b=2,a=1", CollectionShapes.OrderedMapInput(ordered));
        Assert.Equal(3, CollectionShapes.NumberMapInput(numbered));
        Assert.Equal("item7", CollectionShapes.TupleInput(("item", 7)));
        Assert.Equal(15, CollectionShapes.NestedInput(nested));
    }
}
