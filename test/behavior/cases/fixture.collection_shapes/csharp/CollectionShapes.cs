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

    [SpecOperation("lookup", Spec = "fixture.collection_shapes")]
    public static ILookup<string, int> Lookup()
    {
        (string Key, int Value)[] values = [("b", 2), ("a", 1), ("b", 2)];
        return values.ToLookup(value => value.Key, value => value.Value);
    }

    [SpecOperation("number_lookup", Spec = "fixture.collection_shapes")]
    public static ILookup<int, string> NumberLookup()
    {
        (int Key, string Value)[] values = [(2, "two"), (1, "one"), (2, "two")];
        return values.ToLookup(value => value.Key, value => value.Value);
    }

    [SpecOperation("optional_lookup", Spec = "fixture.collection_shapes")]
    public static ILookup<string?, int> OptionalLookup()
    {
        (string? Key, int Value)[] values = [(null, 2), ("a", 1), (null, 2)];
        return values.ToLookup(value => value.Key, value => value.Value);
    }

    [SpecOperation("grouping", Spec = "fixture.collection_shapes")]
    public static IGrouping<string, int> Grouping()
    {
        int[] values = [2, 1, 2];
        return values.GroupBy(_ => "group").Single();
    }

    [SpecOperation("optional_grouping", Spec = "fixture.collection_shapes")]
    public static IGrouping<string?, int> OptionalGrouping()
    {
        int[] values = [2, 1, 2];
        return values.GroupBy(_ => (string?)null).Single();
    }

    [SpecOperation("bag_input", Spec = "fixture.collection_shapes")]
    public static int BagInput([SpecInput("values")] IEnumerable<int> values) => values.Sum();

    [SpecOperation("set_input", Spec = "fixture.collection_shapes")]
    public static int SetInput([SpecInput("values")] ISet<int> values) => values.Sum();

    [SpecOperation("ordered_map_input", Spec = "fixture.collection_shapes")]
    public static string OrderedMapInput([SpecInput("values")] OrderedDictionary<string, int> values) =>
        string.Join(",", values.Select(entry => $"{entry.Key}={entry.Value}"));

    [SpecOperation("number_map_input", Spec = "fixture.collection_shapes")]
    public static int NumberMapInput([SpecInput("values")] Dictionary<int, string> values) => values.Keys.Sum();

    [SpecOperation("tuple_input", Spec = "fixture.collection_shapes")]
    public static string TupleInput([SpecInput("value")] (string, int) value) => value.Item1 + value.Item2;

    [SpecOperation("nested_input", Spec = "fixture.collection_shapes")]
    public static int NestedInput([SpecInput("values")] Dictionary<string, IEnumerable<(int, int)>> values) =>
        values.Values.SelectMany(items => items).Sum(value => value.Item1 + value.Item2);
}
