using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    [Fact]
    public void ScalarAndStructuredRecordsRemainDistinct()
    {
        Assert.Equal(
            9_000_000_000L,
            Assert.IsType<IntegerValue>(ScalarTypes.Classify(9_000_000_000, true)).Value);
        Assert.False(
            Assert.IsType<BooleanValue>(ScalarTypes.Classify(9_000_000_000, false)).Value);
        ScalarAndStructuredRecordsRemainDistinctStep1();
        ScalarAndStructuredRecordsRemainDistinctStep2();
        ScalarAndStructuredRecordsRemainDistinctStep3();
    }
}
