using System.Text.Json.Serialization;
using SpecGate.Annotations;

namespace SpecGateFixtures;

/// <summary>A sensor reading with a scalar, a string, and a numeric list — exercises mixed-type struct serialization.</summary>
[SpecEvent]
public sealed class Measurement
{
    /// <summary>The measured temperature.</summary>
    [SpecEvent("temperature")]
    public int Temperature { get; set; }

    /// <summary>A human-readable sensor label.</summary>
    [SpecEvent("label")]
    public string Label { get; set; } = string.Empty;

    /// <summary>The ordered list of raw readings.</summary>
    [SpecEvent("readings")]
    public List<int> Readings { get; set; } = [];
}

/// <summary>A product record exercising a struct with a nested list and map.</summary>
[SpecEvent]
public sealed class Product
{
    /// <summary>The product name (emitted as <c>product_name</c>).</summary>
    [SpecEvent("product_name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>The product price.</summary>
    [SpecEvent("price")]
    public int Price { get; set; }

    /// <summary>Free-form tags.</summary>
    [SpecEvent("tags")]
    public List<string> Tags { get; set; } = [];

    /// <summary>Key/value attributes, serialized as a canonically ordered map.</summary>
    [SpecEvent("attributes")]
    public Dictionary<string, string> Attributes { get; set; } = [];
}

/// <summary>An entity-type descriptor exercising a struct with multiple string lists.</summary>
[SpecEvent]
public sealed class EntityType
{
    /// <summary>The entity name (emitted as <c>entity_name</c>).</summary>
    [SpecEvent("entity_name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>The names of the entity's key properties.</summary>
    [SpecEvent("key_properties")]
    public List<string> KeyProperties { get; set; } = [];

    /// <summary>The names of all structural properties.</summary>
    [SpecEvent("structural_properties")]
    public List<string> StructuralProperties { get; set; } = [];
}

/// <summary>A two-dimensional offset used as a structured operation input.</summary>
[SpecEvent]
public sealed class Offset
{
    /// <summary>The horizontal delta.</summary>
    [SpecEvent("dx")]
    [JsonPropertyName("dx")]
    public int Dx { get; set; }

    /// <summary>The vertical delta.</summary>
    [SpecEvent("dy")]
    [JsonPropertyName("dy")]
    public int Dy { get; set; }
}

/// <summary>
/// Fixtures returning and consuming structured values — structs, lists, maps,
/// sets, and nested combinations — to exercise the harness's canonical value
/// serialization and structured input materialization.
/// </summary>
public static class ComplexValueFixtures
{
    /// <summary>Returns a fixed sample <see cref="Measurement"/>.</summary>
    /// <returns>A measurement with preset temperature, label, and readings.</returns>
    [SpecOperation("get_measurement")]
    public static Measurement GetMeasurement() =>
        new()
        {
            Temperature = 72,
            Label = "sensor-A3-north",
            Readings = [68, 70, 72, 71, 73],
        };

    /// <summary>Returns an empty string list, exercising empty-collection serialization.</summary>
    /// <returns>An empty <see cref="List{T}"/> of strings.</returns>
    [SpecOperation("get_empty")]
    public static List<string> GetEmpty() => [];

    /// <summary>Returns a fixed sample <see cref="Product"/>.</summary>
    /// <returns>A product with preset name, price, tags, and attributes.</returns>
    [SpecOperation("get_product")]
    public static Product GetProduct() =>
        new()
        {
            Name = "Milk",
            Price = 4,
            Tags = ["dairy", "organic", "local"],
            Attributes = new Dictionary<string, string>
            {
                ["category"] = "food",
                ["origin"] = "local",
            },
        };

    /// <summary>Returns a fixed sample <see cref="EntityType"/>.</summary>
    /// <returns>An entity type describing a <c>Customer</c>.</returns>
    [SpecOperation("resolve_entity")]
    public static EntityType ResolveEntity() =>
        new()
        {
            Name = "Customer",
            KeyProperties = ["ID"],
            StructuralProperties = ["ID", "Name", "Email"],
        };

    /// <summary>Returns a map of entity field values keyed by field name.</summary>
    /// <param name="id">The entity id, formatted into the returned map (spec input <c>id</c>).</param>
    /// <returns>A dictionary of the entity's field values.</returns>
    [SpecOperation("get_entity_values")]
    public static Dictionary<string, string> GetEntityValues([SpecInput("id")] int id) =>
        new()
        {
            ["ID"] = id.ToString(System.Globalization.CultureInfo.InvariantCulture),
            ["Name"] = "Customer",
            ["Email"] = "cust@example.com",
        };

    /// <summary>Returns navigation property names, exercising set serialization (sorted, deduplicated).</summary>
    /// <returns>A <see cref="SortedSet{T}"/> of navigation property names.</returns>
    [SpecOperation("get_navigation_properties")]
    public static SortedSet<string> GetNavigationProperties() =>
        ["Orders", "Address", "Contacts"];

    /// <summary>Returns a list of property descriptor maps, exercising nested list-of-map serialization.</summary>
    /// <returns>A list where each entry is a name/type/nullable descriptor map.</returns>
    [SpecOperation("get_properties")]
    public static List<Dictionary<string, string>> GetProperties() =>
        [
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
        ];

    /// <summary>Multiplies <paramref name="value"/> by <paramref name="factor"/>.</summary>
    /// <param name="value">The base value (spec input <c>value</c>).</param>
    /// <param name="factor">The multiplier (spec input <c>factor</c>).</param>
    /// <returns>The product <paramref name="value"/> * <paramref name="factor"/>.</returns>
    [SpecOperation("scale")]
    public static int Scale([SpecInput("value")] int value, [SpecInput("factor")] int factor) => value * factor;

    /// <summary>Shifts <paramref name="base"/> by a structured <see cref="Offset"/> input.</summary>
    /// <param name="base">The base value (spec input <c>base</c>).</param>
    /// <param name="by">The offset to apply (spec input <c>by</c>).</param>
    /// <returns><paramref name="base"/> plus the offset's <see cref="Offset.Dx"/> and <see cref="Offset.Dy"/>.</returns>
    [SpecOperation("shift")]
    public static int Shift([SpecInput("base")] int @base, [SpecInput("by")] Offset by) => @base + by.Dx + by.Dy;
}
