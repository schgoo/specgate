using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    [Fact]
    public void StatelessAndMultiOperationFixturesPreserveArithmetic()
    {
        Assert.Equal(5, StatelessAdd.Add(2, 3));
        Assert.Equal(-5, StatelessAdd.Add(-8, 3));
        StatelessAndMultiOperationFixturesPreserveArithmeticStep1();
        StatelessAndMultiOperationFixturesPreserveArithmeticStep2();
    }
}
