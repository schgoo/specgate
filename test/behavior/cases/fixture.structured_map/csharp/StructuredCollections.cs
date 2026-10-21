using System.Globalization;
using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

public static partial class StructuredCollections
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
}
