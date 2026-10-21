using SpecGate.CtscFixtures.Conformance.SumTypes;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class SumTypeBehaviorTests
{
    private static void DeclaredErrorsRemainDistinctFromUndeclaredFaultsStep1()
    {
        Assert.Equal(40, DeclaredErrors.RequireInRange(40));
        Assert.IsType<InvalidOperationException>(
            Assert.ThrowsAny<Exception>(() => DeclaredErrors.RequireInRange(-1)));
        Assert.IsType<FormatException>(
            Assert.ThrowsAny<Exception>(() => DeclaredErrors.RequireInRange(101)));
    }
}
