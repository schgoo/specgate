using SpecGate.CtscFixtures.Conformance.Structured;
using Xunit;

namespace SpecGate.CtscFixtures.Tests;

public sealed partial class StructuredBehaviorTests
{
    private static void ScalarAndStructuredRecordsRemainDistinctStep1()
    {
        Measurement measurement = ScalarOperators.GetMeasurement();
        Assert.Equal(72, measurement.Temperature);
        Assert.StartsWith("sensor-", measurement.Label, StringComparison.Ordinal);
        Assert.Equal(5, measurement.Readings.Count);
        Assert.Empty(ScalarOperators.GetEmpty());
    }
}
