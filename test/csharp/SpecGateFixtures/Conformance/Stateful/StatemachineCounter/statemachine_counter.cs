using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.StatemachineCounter;

/// <summary>Single-step state machine: a counter mutated by <c>increment</c>.</summary>
public class Counter
{
    /// <summary>The current count; each assignment is captured as a state mutation.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a counter starting at zero.</summary>
    /// <returns>A new <see cref="Counter"/> with <see cref="Count"/> 0.</returns>
    [SpecSetup("increment", Spec = "fixture.statemachine_counter")]
    public static Counter Make() => new() { Count = 0 };

    /// <summary>Increments the counter by one.</summary>
    [SpecOperation("increment", Spec = "fixture.statemachine_counter")]
    public void Increment() => Count += 1;
}
