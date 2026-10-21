using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    private static void DeclaredErrorsRemainDistinctFromUndeclaredFaultsStep2()
    {
        Assert.Equal(
            "attempt to divide by zero",
            Assert.Throws<InvalidOperationException>(
                () => DeclaredErrors.Divide(1, 0)).Message);
    }
}
