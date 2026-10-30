using System.Collections;
using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Collections;

[SpecEvent("CollectionItem")]
public sealed class CollectionItem
{
    [SpecEvent("value")]
    public int Value { get; init; }
}

public sealed class ItemEnumerable : IEnumerable<CollectionItem>
{
    IEnumerator<CollectionItem> IEnumerable<CollectionItem>.GetEnumerator()
    {
        yield return new CollectionItem { Value = 2 };
        yield return new CollectionItem { Value = 1 };
        yield return new CollectionItem { Value = 2 };
    }

    IEnumerator IEnumerable.GetEnumerator() => ((IEnumerable<CollectionItem>)this).GetEnumerator();
}

public static class CollectionShapes
{
    [SpecOperation("items", Spec = "fixture.collection_shapes")]
    public static Task<IEnumerable<CollectionItem>> ItemsAsync() =>
        Task.FromResult<IEnumerable<CollectionItem>>(new ItemEnumerable());

    [SpecOperation("ordered_items", Spec = "fixture.collection_shapes")]
    public static IOrderedEnumerable<int> OrderedItems()
    {
        int[] values = [2, 1, 2];
        return values.OrderBy(value => value);
    }

    [SpecOperation("ordered_map", Spec = "fixture.collection_shapes")]
    public static OrderedDictionary<string, int> OrderedMap() => new() { ["b"] = 2, ["a"] = 1 };

    [SpecOperation("priorities", Spec = "fixture.collection_shapes")]
    public static PriorityQueue<string, int> Priorities()
    {
        PriorityQueue<string, int> queue = new();
        queue.Enqueue("later", 7);
        queue.Enqueue("first", 1);
        return queue;
    }
}
