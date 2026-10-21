using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    private static void OptionAndResultSurfacesCoverBothArmsStep3()
    {
        Result<int, string> failure = OptionResult.TryDivideErr(12, 0);
        Assert.False(failure.IsOk);
        Assert.Equal("division by zero", failure.ErrValue);
    }
}
