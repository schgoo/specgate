using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

public static partial class StructuredCollections
{
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
