using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    [Fact]
    public void NamedInputsAndCheckpointBehaviorRemainOrdinaryMethods()
    {
        Assert.Equal(5, NamedInputs.Divide(20, 4));

        Scaler scaler = Scaler.Make(3);
        Assert.Equal(21, scaler.Scale(7));
        NamedInputsAndCheckpointBehaviorRemainOrdinaryMethodsStep1();
    }
}
