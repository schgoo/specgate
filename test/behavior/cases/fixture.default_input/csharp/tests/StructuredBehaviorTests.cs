using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    [Fact]
    public void FormerDefaultInputsAreExplicitOperationInputs()
    {
        Assert.Equal(12, DefaultInput.Scale(6, 2));
        Assert.Equal(12, DefaultInput.Shift(10, new Offset { Dx = 1, Dy = 1 }));
    }
}
