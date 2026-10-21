using SpecGate.Annotations;
using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    [Fact]
    public void OptionAndResultSurfacesCoverBothArms()
    {
        Option<int> found = OptionResult.FindSome([4, 7, 9], 7);
        Assert.True(found.HasValue);
        Assert.Equal(1, found.Value);
        OptionAndResultSurfacesCoverBothArmsStep1();
        OptionAndResultSurfacesCoverBothArmsStep2();
        OptionAndResultSurfacesCoverBothArmsStep3();
    }
}
