using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    [Fact]
    public void StatelessAndMultiOperationFixturesPreserveArithmetic()
    {
        Assert.Equal(5, StatelessAdd.Add(2, 3, TestContext.Current.CancellationToken));
        Assert.Equal(-5, StatelessAdd.Add(-8, 3, new CancellationToken(canceled: true)));
        StatelessAndMultiOperationFixturesPreserveArithmeticStep1();
        StatelessAndMultiOperationFixturesPreserveArithmeticStep2();
    }
}
