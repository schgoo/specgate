using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

[SpecEvent("Measurement")]
public sealed class Measurement
{
    [SpecEvent("temperature")]
    public int Temperature { get; set; }

    [SpecEvent("label")]
    public string Label { get; set; } = string.Empty;

    [SpecEvent("readings")]
    public List<int> Readings { get; set; } = [];
}

public static class ScalarOperators
{
    [SpecOperation("get_measurement", Spec = "fixture.scalar_operators")]
    public static Measurement GetMeasurement() =>
        new()
        {
            Temperature = 72,
            Label = "sensor-A3-north",
            Readings = [68, 70, 72, 71, 73],
        };

    [SpecOperation("get_empty", Spec = "fixture.scalar_operators")]
    public static List<string> GetEmpty() => [];
}
