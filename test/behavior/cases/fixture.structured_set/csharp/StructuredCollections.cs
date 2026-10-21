using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

public static partial class StructuredCollections
{
    [SpecOperation("get_navigation_properties", Spec = "fixture.structured_set")]
    public static SortedSet<string> GetNavigationProperties() =>
        ["Orders", "Address", "Contacts"];
}
