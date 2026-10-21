using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

[SpecEvent("Product")]
public sealed class Product
{
    [SpecEvent("product_name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("price")]
    public int Price { get; set; }

    [SpecEvent("tags")]
    public List<string> Tags { get; set; } = [];

    [SpecEvent("attributes")]
    public SortedDictionary<string, string> Attributes { get; set; } = [];
}

public static class Operators
{
    [SpecOperation("get_product", Spec = "fixture.operators")]
    public static Product GetProduct() =>
        new()
        {
            Name = "Milk",
            Price = 4,
            Tags = ["dairy", "organic", "local"],
            Attributes = new SortedDictionary<string, string>
            {
                ["category"] = "food",
                ["origin"] = "local",
            },
        };
}
