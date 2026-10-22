using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.MultiMutation;

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment_twice", Spec = "fixture.multi_mutation")]
    public static Counter Make() => new();

    [SpecOperation("increment_twice", Spec = "fixture.multi_mutation")]
    public void IncrementTwice()
    {
        Count++;
        _ = ObserveCount("after_first", Count);
        Count++;
        _ = ObserveCount("after_second", Count);
    }

    [SpecOperation("observe_count", Spec = "fixture.multi_mutation")]
    public static int ObserveCount(
        [SpecInput("stage")] string stage,
        [SpecInput("count")] int count)
    {
        ArgumentOutOfRangeException.ThrowIfNotEqual(
            stage is "after_first" or "after_second",
            true);
        return count;
    }
}
