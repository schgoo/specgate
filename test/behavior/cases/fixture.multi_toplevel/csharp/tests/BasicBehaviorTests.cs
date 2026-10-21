using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    private static void StatelessAndMultiOperationFixturesPreserveArithmeticStep2()
    {
        Assert.Equal(5, MultiOperation.Alpha(4));
        Assert.Equal(8, MultiOperation.Beta(4));
    }
}
