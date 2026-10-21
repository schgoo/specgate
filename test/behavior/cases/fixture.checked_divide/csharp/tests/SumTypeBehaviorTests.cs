using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    [Fact]
    public void DeclaredErrorsRemainDistinctFromUndeclaredFaults()
    {
        Assert.Equal(4, DeclaredErrors.CheckedDivide(12, 3));
        Assert.Equal(
            "division by zero",
            Assert.Throws<DivideByZeroException>(
                () => DeclaredErrors.CheckedDivide(12, 0)).Message);
        Assert.Equal(
            "negative divisor",
            Assert.Throws<InvalidOperationException>(
                () => DeclaredErrors.CheckedDivide(12, -3)).Message);
        DeclaredErrorsRemainDistinctFromUndeclaredFaultsStep1();
        DeclaredErrorsRemainDistinctFromUndeclaredFaultsStep2();
    }
}
