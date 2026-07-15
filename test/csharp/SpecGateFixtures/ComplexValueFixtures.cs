using System.Collections.Generic;
using System.Text.Json.Serialization;
using SpecGate.Annotations;

namespace SpecGateFixtures;

[SpecEvent]
public sealed class Measurement
{
    [SpecEvent("temperature")]
    public int Temperature { get; set; }

    [SpecEvent("label")]
    public string Label { get; set; } = string.Empty;

    [SpecEvent("readings")]
    public List<int> Readings { get; set; } = new();
}

[SpecEvent]
public sealed class Product
{
    [SpecEvent("product_name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("price")]
    public int Price { get; set; }

    [SpecEvent("tags")]
    public List<string> Tags { get; set; } = new();

    [SpecEvent("attributes")]
    public Dictionary<string,string> Attributes { get; set; } = new();
}

[SpecEvent]
public sealed class EntityType
{
    [SpecEvent("entity_name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("key_properties")]
    public List<string> KeyProperties { get; set; } = new();

    [SpecEvent("structural_properties")]
    public List<string> StructuralProperties { get; set; } = new();
}

public sealed class Offset
{
    [JsonPropertyName("dx")]
    public int Dx { get; set; }

    [JsonPropertyName("dy")]
    public int Dy { get; set; }
}

public static class ComplexValueFixtures
{
    [SpecOperation("get_measurement")]
    public static Measurement GetMeasurement() =>
        new()
        {
            Temperature = 72,
            Label = "sensor-A3-north",
            Readings = new List<int> { 68, 70, 72, 71, 73 },
        };

    [SpecOperation("get_empty")]
    public static List<string> GetEmpty() => new();

    [SpecOperation("get_product")]
    public static Product GetProduct() =>
        new()
        {
            Name = "Milk",
            Price = 4,
            Tags = new List<string> { "dairy", "organic", "local" },
            Attributes = new Dictionary<string,string>
            {
                ["category"] = "food",
                ["origin"] = "local",
            },
        };

    [SpecOperation("resolve_entity")]
    public static EntityType ResolveEntity() =>
        new()
        {
            Name = "Customer",
            KeyProperties = new List<string> { "ID" },
            StructuralProperties = new List<string> { "ID", "Name", "Email" },
        };

    [SpecOperation("get_entity_values")]
    public static Dictionary<string,string> GetEntityValues([SpecInput("id")] int id) =>
        new()
        {
            ["ID"] = id.ToString(System.Globalization.CultureInfo.InvariantCulture),
            ["Name"] = "Customer",
            ["Email"] = "cust@example.com",
        };

    [SpecOperation("get_navigation_properties")]
    public static SortedSet<string> GetNavigationProperties() =>
        new() { "Orders", "Address", "Contacts" };

    [SpecOperation("get_properties")]
    public static List<Dictionary<string,string>> GetProperties() =>
        new()
        {
            new Dictionary<string,string>
            {
                ["name"] = "ID",
                ["type"] = "Edm.Int32",
                ["nullable"] = "false",
            },
            new Dictionary<string,string>
            {
                ["name"] = "Name",
                ["type"] = "Edm.String",
                ["nullable"] = "true",
            },
        };

    [SpecOperation("scale")]
    public static int Scale([SpecInput("value")] int value, [SpecInput("factor")] int factor) => value * factor;

    [SpecOperation("shift")]
    public static int Shift([SpecInput("base")] int @base, [SpecInput("by")] Offset by) => @base + by.Dx + by.Dy;
}
