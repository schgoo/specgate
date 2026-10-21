using Xunit;
using MultiSetup = SpecGate.CtscFixtures.Conformance.Stateful.MultiSetup;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    [Fact]
    public void MultipleAndSharedSetupsFillDistinctParameters()
    {
        MultiSetup.Account source = MultiSetup.Transfer.MakeSource();
        MultiSetup.Account target = MultiSetup.Transfer.MakeTarget();
        MultiSetup.Transfer.Execute(source, target, 30);
        Assert.Equal(70, source.Balance);
        Assert.Equal(30, target.Balance);
        MultipleAndSharedSetupsFillDistinctParametersStep1();
    }
}
