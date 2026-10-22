using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    [Fact]
    public void EveryPayloadFreeEnumVariantIsReachable()
    {
        Assert.Equal(Level.Low, PlainLevels.LevelFor(0));
        Assert.Equal(Level.Medium, PlainLevels.LevelFor(50));
        Assert.Equal(Level.High, PlainLevels.LevelFor(500));
    }
}
