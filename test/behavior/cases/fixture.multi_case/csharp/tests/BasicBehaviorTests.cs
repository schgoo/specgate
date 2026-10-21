using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    private static void StatelessAndMultiOperationFixturesPreserveArithmeticStep1()
    {
        Assert.Equal(5, MultiCase.Add(2, 3));
        Assert.Equal(-5, MultiCase.Add(-8, 3));
    }
}
