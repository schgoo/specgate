using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    private static void ScalarAndStructuredRecordsRemainDistinctStep3()
    {
        EntityType entity = StructuredOutput.ResolveEntity();
        Assert.Equal("Customer", entity.Name);
        Assert.Equal(["ID"], entity.KeyProperties);
        Assert.Equal(["ID", "Name", "Email"], entity.StructuralProperties);
    }
}
