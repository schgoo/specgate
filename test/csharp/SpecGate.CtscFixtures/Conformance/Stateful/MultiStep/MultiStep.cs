using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.MultiStep;

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.multi_step")]
    [SpecSetup("decrement", Spec = "fixture.multi_step")]
    public static Counter Make() => new();

    [SpecOperation("increment", Spec = "fixture.multi_step")]
    public void Increment()
    {
        Count++;
        _ = ObserveCount("increment", Count);
    }

    [SpecOperation("decrement", Spec = "fixture.multi_step")]
    public void Decrement()
    {
        Count--;
        _ = ObserveCount("decrement", Count);
    }

    [SpecOperation("observe_count", Spec = "fixture.multi_step")]
    public static int ObserveCount(
        [SpecInput("operation")] string operation,
        [SpecInput("count")] int count)
    {
        ArgumentOutOfRangeException.ThrowIfNotEqual(
            operation is "increment" or "decrement",
            true);
        return count;
    }
}
