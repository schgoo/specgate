using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.SetupWithParams;

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.setup_with_params")]
    public static Counter Make([SpecInput("initial")] int initial) =>
        new() { Count = initial };

    [SpecOperation("increment", Spec = "fixture.setup_with_params")]
    public void Increment()
    {
        Count++;
        _ = ObserveCount(Count);
    }

    [SpecOperation("observe_count", Spec = "fixture.setup_with_params")]
    public static int ObserveCount([SpecInput("count")] int count) => count;
}
