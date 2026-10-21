using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    private static void MapsSetsAndNestedCollectionsAreDeterministicStep2()
    {
        List<SortedDictionary<string, string>> properties =
            StructuredCollections.GetProperties();
        Assert.Equal("ID", properties[0]["name"]);
        Assert.Equal("true", properties[1]["nullable"]);
    }
}
