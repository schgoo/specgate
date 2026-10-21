using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    private static void ScalarAndStructuredRecordsRemainDistinctStep2()
    {
        Product product = Operators.GetProduct();
        Assert.Equal("Milk", product.Name);
        Assert.Contains("organic", product.Tags);
        Assert.Equal("local", product.Attributes["origin"]);
    }
}
