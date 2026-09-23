using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.StatemachineCounter;

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.statemachine_counter")]
    public static Counter Make() => new();

    [SpecOperation("increment", Spec = "fixture.statemachine_counter")]
    public void Increment()
    {
        Count++;
        _ = ObserveState(this);
    }

    [SpecOperation("observe_state", Spec = "fixture.statemachine_counter")]
    public static Counter ObserveState([SpecInput("counter")] Counter counter) =>
        new() { Count = counter.Count };
}
