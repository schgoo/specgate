using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.PropertySetups;

[SpecEvent("Query", Setup = true)]
public sealed class Query
{
    public string filter { get; set; } = "";
    public Option<uint> top { get; set; }
    public List<string> tags { get; set; } = [];
    public SortedSet<string> allowed { get; set; } = [];
    public SortedDictionary<string, Option<List<int>>> weights { get; set; } = [];
}

[SpecEvent("RenamedSubset", Setup = true)]
public sealed class Subset
{
    [SpecEvent("limit")]
    public uint Selected { get; set; }
    public string Cache { get; set; } = "";
}

[SpecEvent("Empty", Setup = true)]
public sealed class Empty;

[SpecEvent("Fields", Setup = true)]
public sealed class Fields
{
    public Query query { get; set; } = new();
    public Subset subset { get; set; } = new();
    public Empty empty { get; set; } = new();
}

[SpecEvent("Request", Setup = true)]
public sealed class Request
{
    public Fields fields { get; set; } = new();
}

public class ConstructorBase
{
    protected ConstructorBase() { }
}

[SpecEvent("Constructed")]
public sealed class Constructed : ConstructorBase
{
    [SpecEvent("filter")]
    public string Filter { get; }

    [SpecSetup("constructor", Fills = "query", Spec = "fixture.property_setup")]
    public Constructed([SpecInput("filter")] string filter)
    {
        if (filter == "throw") throw new ArgumentException("intentional constructor failure", nameof(filter));
        Filter = filter;
    }

    [SpecSetup("nested_constructor", Fills = "query", Spec = "fixture.property_setup")]
    public Constructed([SpecInput("options")] Request options)
    {
        Filter = $"{options.fields.query.filter}:{options.fields.subset.Selected}";
    }
}

[SpecEvent("NestedCounter")]
public sealed class NestedCounter
{
    [SpecEvent("count")]
    public uint Count { get; set; }

    [SpecSetup("nested_increment", Spec = "fixture.property_setup")]
    public NestedCounter([SpecInput("options")] Request options)
    {
        Count = options.fields.subset.Selected;
        options.fields.subset.Selected++;
    }

    [SpecOperation("nested_increment", Spec = "fixture.property_setup")]
    public uint Increment() => ++Count;
}

public static class Operations
{
    [SpecOperation("nested_fields", Spec = "fixture.property_setup")]
    public static string NestedFields([SpecInput("query")] Request query)
    {
        string result = $"{query.fields.query.filter}:{query.fields.subset.Selected}";
        query.fields.subset.Selected++;
        return result;
    }

    [SpecOperation("nested_constructor", Spec = "fixture.property_setup")]
    public static string NestedConstructor([SpecInput("query")] Constructed query) => query.Filter;

    [SpecOperation("zero_inputs", Spec = "fixture.property_setup")]
    public static int ZeroInputs([SpecInput("_query")] Empty query) => 3;

    [SpecSetup("producer_precedence", Fills = "query", Spec = "fixture.property_setup")]
    public static Query Produce([SpecInput("filter")] string filter) => new() { filter = filter };

    [SpecOperation("producer_precedence", Spec = "fixture.property_setup")]
    public static string ProducerPrecedence([SpecInput("query")] Query query) => query.filter;

    [SpecOperation("constructor", Spec = "fixture.property_setup")]
    public static string Constructor([SpecInput("query")] Constructed query) => query.Filter;

    [SpecOperation("all_fields", Spec = "fixture.property_setup")]
    public static string AllFields([SpecInput("query")] Query query) =>
        $"{query.filter}:{(query.top.HasValue ? query.top.Value : 0)}:"
        + $"{string.Join(",", query.tags)}:{string.Join(",", query.allowed)}:"
        + $"{query.weights.Values.Where(value => value.HasValue).SelectMany(value => value.Value!).Sum()}";

    [SpecOperation("subset_renamed", Spec = "fixture.property_setup")]
    public static uint SubsetRenamed([SpecInput("query")] Subset query, [SpecInput("selected")] uint selected)
    {
        uint result = query.Selected + selected;
        query.Selected++;
        return result;
    }

    [SpecOperation("module_relative", Spec = "fixture.property_setup")]
    public static string ModuleRelative([SpecInput("query")] Query query) => query.filter;
}
