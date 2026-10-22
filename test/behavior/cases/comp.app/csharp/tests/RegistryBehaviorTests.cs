using Xunit;
using Ownership = SpecGate.CtscFixtures.Registry.Components.ComponentOwnership;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class RegistryBehaviorTests
{
    private static void ComponentOwnershipAndDivergenceWitnessAreExplicitStep1()
    {
        Assert.Equal("assembled", Ownership.Assemble().Label);
    }
}
