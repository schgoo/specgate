using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    private static void MapsSetsAndNestedCollectionsAreDeterministicStep1()
    {
        Assert.Equal(
            ["Address", "Contacts", "Orders"],
            StructuredCollections.GetNavigationProperties());
    }
}
