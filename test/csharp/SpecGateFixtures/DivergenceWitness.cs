using SpecGate.Annotations;

namespace SpecGateFixtures;

// Multi-target divergence witness (C# side). Agrees with the Rust fixture on
// `value` (10) but deliberately reports a different `engine`, so the two
// targets satisfy the same `expected:` while emitting different traces.
[SpecEvent]
public sealed class EngineInfo
{
    [SpecEvent("value")]
    public int Value { get; set; }

    [SpecEvent("engine")]
    public string Engine { get; set; } = string.Empty;
}

public static class DivergenceWitness
{
    [SpecOperation("engine_info")]
    public static EngineInfo GetEngineInfo() =>
        new() { Value = 10, Engine = "csharp" };
}
