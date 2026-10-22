using Xunit;
using Shared = SpecGate.CtscFixtures.Conformance.Stateful.SharedSetup;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StatefulBehaviorTests
{
    private static void MultipleAndSharedSetupsFillDistinctParametersStep1()
    {
        Shared.BoxVal left = Shared.Combine.MakeBox();
        Shared.BoxVal right = Shared.Combine.MakeBox();
        Assert.Equal(4, Shared.Combine.Two(left, right));
        Assert.Equal(
            3,
            Shared.Combine.Three(
                Shared.Combine.MakeUnit(),
                Shared.Combine.MakeUnit(),
                Shared.Combine.MakeUnit()));
    }
}
