using SpecGate.Annotations;

namespace SpecGateFixtures.Stateful.MultiStep;

/// <summary>Multi-step state machine: a counter driven through <c>increment</c> then <c>decrement</c>.</summary>
public class Counter
{
    /// <summary>The current count; each assignment is captured as a state mutation.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a counter starting at zero.</summary>
    /// <returns>A new <see cref="Counter"/> with <see cref="Count"/> 0.</returns>
    [SpecSetup("increment")]
    public static Counter Make() => new() { Count = 0 };

    /// <summary>Increments the counter by one.</summary>
    [SpecOperation("increment")]
    public void Increment() => Count += 1;

    /// <summary>Decrements the counter by one.</summary>
    [SpecOperation("decrement")]
    public void Decrement() => Count -= 1;
}
