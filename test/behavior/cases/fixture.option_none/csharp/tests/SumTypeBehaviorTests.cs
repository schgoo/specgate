using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    private static void OptionAndResultSurfacesCoverBothArmsStep1()
    {
        Assert.False(OptionResult.FindNone([4, 7, 9], 8).HasValue);
    }
}
