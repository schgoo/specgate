using SpecGate.CtscFixtures.Conformance.Basic;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed class UnsignedBehaviorTests
{
    [Fact]
    public void UnsignedBoundariesRoundTrip()
    {
        foreach (uint value in new uint[] { 0, int.MaxValue, 2_147_483_648, uint.MaxValue })
        {
            Assert.Equal(value, BoundaryEchoes.EchoU32(value));
        }

        foreach (ulong value in new ulong[] { 0, long.MaxValue, 9_223_372_036_854_775_808, ulong.MaxValue })
        {
            Assert.Equal(value, BoundaryEchoes.EchoU64(value));
        }
    }
}
