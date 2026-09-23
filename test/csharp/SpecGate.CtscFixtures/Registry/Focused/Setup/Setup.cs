using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Registry.Focused.Setup;

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.setup")]
    public static Counter Make([SpecInput("initial")] int initial) =>
        new() { Count = initial };

    [SpecOperation("increment", Spec = "fixture.setup")]
    public void Increment()
    {
        Count++;
        _ = ObserveCount(Count);
    }

    [SpecOperation("observe_count", Spec = "fixture.setup")]
    public static int ObserveCount([SpecInput("count")] int count) => count;
}
