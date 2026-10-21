using Xunit;
using FarewellFixture = SpecGate.CtscFixtures.Conformance.MultiFile.Farewell;
using GreetFixture = SpecGate.CtscFixtures.Conformance.MultiFile.Greet;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class BasicBehaviorTests
{
    [Fact]
    public void MultiFileOperationsRemainIndependent()
    {
        Assert.Equal("Hello, Ada!", GreetFixture.Execute("Ada"));
        Assert.Equal("Goodbye, Ada!", FarewellFixture.Execute("Ada"));
    }
}
