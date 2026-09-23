using System.Globalization;
using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

public static class StructuredCollections
{
    [SpecOperation("get_entity_values", Spec = "fixture.structured_map")]
    public static SortedDictionary<string, string> GetEntityValues(
        [SpecInput("id")] int id) =>
        new()
        {
            ["ID"] = id.ToString(CultureInfo.InvariantCulture),
            ["Name"] = "Customer",
            ["Email"] = "cust@example.com",
        };

    [SpecOperation("get_navigation_properties", Spec = "fixture.structured_set")]
    public static SortedSet<string> GetNavigationProperties() =>
        ["Orders", "Address", "Contacts"];

    [SpecOperation("get_properties", Spec = "fixture.nested_structured")]
    public static List<SortedDictionary<string, string>> GetProperties() =>
        [
            new()
            {
                ["name"] = "ID",
                ["type"] = "Edm.Int32",
                ["nullable"] = "false",
            },
            new()
            {
                ["name"] = "Name",
                ["type"] = "Edm.String",
                ["nullable"] = "true",
            },
        ];
}
