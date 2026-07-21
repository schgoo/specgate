using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.MultiMutation;

/// <summary>
/// net8.0 copy of the multi-mutation counter fixture — the cross-framework
/// witness proving intermediate-mutation capture works on an older TFM.
/// </summary>
public class Counter
{
    /// <summary>The current count; each assignment is captured as a distinct mutation.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a counter starting at zero.</summary>
    /// <returns>A new <see cref="Counter"/> with <see cref="Count"/> 0.</returns>
    [SpecSetup("increment_twice")]
    public static Counter Make() => new() { Count = 0 };

    /// <summary>Increments the counter twice, emitting both intermediate values.</summary>
    [SpecOperation("increment_twice")]
    public void IncrementTwice()
    {
        Count += 1;
        Count += 1;
    }
}
