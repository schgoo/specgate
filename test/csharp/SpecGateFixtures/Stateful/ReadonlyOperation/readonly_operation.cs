using SpecGate.Annotations;

namespace SpecGateFixtures.Stateful.ReadonlyOperation;

/// <summary>
/// Read-only operation: <c>get_count</c> returns state without mutating it, so
/// the trace shows the initial state and a <c>$result</c> but no after-state.
/// </summary>
public class Counter
{
    /// <summary>The current count; read but never mutated by the operation.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a counter preset to 42.</summary>
    /// <returns>A new <see cref="Counter"/> with <see cref="Count"/> 42.</returns>
    [SpecSetup("get_count")]
    public static Counter Make() => new() { Count = 42 };

    /// <summary>Returns the current count without changing it.</summary>
    /// <returns>The value of <see cref="Count"/>.</returns>
    [SpecOperation("get_count")]
    public int GetCount() => Count;
}
