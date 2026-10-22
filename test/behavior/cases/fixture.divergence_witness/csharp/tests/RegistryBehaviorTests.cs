using Xunit;
using Witness = SpecGate.CtscFixtures.Conformance.Witness.DivergenceWitness;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class RegistryBehaviorTests
{
    private static void ComponentOwnershipAndDivergenceWitnessAreExplicitStep2()
    {
        Conformance.Witness.EngineInfo info = Witness.EngineInfo();
        Assert.Equal(10, info.Value);
        Assert.Equal("csharp", info.Engine);
    }
}
