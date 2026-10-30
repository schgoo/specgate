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
}
