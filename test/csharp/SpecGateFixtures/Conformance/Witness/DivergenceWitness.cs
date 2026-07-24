using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Witness;

/// <summary>
/// Multi-target divergence witness (C# side). Agrees with the Rust fixture on
/// <see cref="Value"/> (10) but deliberately reports a different
/// <see cref="Engine"/>, so the two targets satisfy the same <c>expected:</c>
/// assertions while emitting different traces — proving divergence is detected.
/// </summary>
[SpecEvent]
public sealed class EngineInfo
{
    /// <summary>A value both targets agree on (10).</summary>
    [SpecEvent("value")]
    public int Value { get; set; }

    /// <summary>The engine name; intentionally target-specific to force divergence.</summary>
    [SpecEvent("engine")]
    public string Engine { get; set; } = string.Empty;
}

/// <summary>Fixture returning the divergence-witness <see cref="EngineInfo"/>.</summary>
public static class DivergenceWitness
{
    /// <summary>Returns engine info with the C#-specific engine name.</summary>
    /// <returns>An <see cref="EngineInfo"/> with value 10 and engine <c>"csharp"</c>.</returns>
    [SpecOperation("engine_info", Spec = "fixture.divergence_witness")]
    public static EngineInfo GetEngineInfo() =>
        new() { Value = 10, Engine = "csharp" };
}
