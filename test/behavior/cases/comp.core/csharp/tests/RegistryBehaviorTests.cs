using Xunit;
using Ownership = SpecGate.CtscFixtures.Registry.Components.ComponentOwnership;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class RegistryBehaviorTests
{
    [Fact]
    public void ComponentOwnershipAndDivergenceWitnessAreExplicit()
    {
        Assert.Equal("widget", Ownership.MakeWidget().Label);
        ComponentOwnershipAndDivergenceWitnessAreExplicitStep1();
        ComponentOwnershipAndDivergenceWitnessAreExplicitStep2();
    }
}
