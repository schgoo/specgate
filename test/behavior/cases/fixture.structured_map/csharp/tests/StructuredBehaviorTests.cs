using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    [Fact]
    public void MapsSetsAndNestedCollectionsAreDeterministic()
    {
        SortedDictionary<string, string> values =
            StructuredCollections.GetEntityValues(12);
        Assert.Equal("12", values["ID"]);
        Assert.Equal("cust@example.com", values["Email"]);
        MapsSetsAndNestedCollectionsAreDeterministicStep1();
        MapsSetsAndNestedCollectionsAreDeterministicStep2();
    }
}
