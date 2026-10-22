using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Witness;

[SpecEvent("EngineInfo")]
public sealed class EngineInfo
{
    [SpecEvent("value")]
    public int Value { get; set; }

    [SpecEvent("engine")]
    public string Engine { get; set; } = string.Empty;
}

public static class DivergenceWitness
{
    [SpecOperation("engine_info", Spec = "fixture.divergence_witness")]
    public static EngineInfo EngineInfo() => new() { Value = 10, Engine = "csharp" };
}
