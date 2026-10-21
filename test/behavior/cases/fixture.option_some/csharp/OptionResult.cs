using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class OptionResult
{
    [SpecOperation("find", Spec = "fixture.option_some")]
    public static Option<int> FindSome(
        [SpecInput("items")] List<int> items,
        [SpecInput("target")] int target) =>
        Locate(items, target);

    private static Option<int> Locate(List<int> items, int target)
    {
        int index = items.IndexOf(target);
        return index < 0 ? Option<int>.None() : Option<int>.Some(index);
    }
}
