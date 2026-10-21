using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class OptionResult
{
    [SpecOperation("find", Spec = "fixture.option_none")]
    public static Option<int> FindNone(
        [SpecInput("items")] List<int> items,
        [SpecInput("target")] int target) =>
        Locate(items, target);
}
