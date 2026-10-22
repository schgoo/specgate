using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    private static void OptionAndResultSurfacesCoverBothArmsStep2()
    {
        Result<int, string> success = OptionResult.TryDivideOk(12, 4);
        Assert.True(success.IsOk);
        Assert.Equal(3, success.OkValue);
    }
}
