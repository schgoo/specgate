using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.MultiMutation;

/// <summary>
/// State machine whose operation mutates the same field twice, verifying that
/// every intermediate mutation is captured (not just the boundary values).
/// </summary>
public class Counter
{
    /// <summary>The current count; each assignment is captured as a distinct mutation.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a counter starting at zero.</summary>
    /// <returns>A new <see cref="Counter"/> with <see cref="Count"/> 0.</returns>
    [SpecSetup("increment_twice", Spec = "fixture.multi_mutation")]
    public static Counter Make() => new() { Count = 0 };

    /// <summary>Increments the counter twice, emitting both intermediate values.</summary>
    [SpecOperation("increment_twice", Spec = "fixture.multi_mutation")]
    public void IncrementTwice()
    {
        Count += 1;
        Count += 1;
    }
}
