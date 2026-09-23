using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static class OptionResult
{
    [SpecOperation("find", Spec = "fixture.option_some")]
    [SpecOperation("find", Spec = "fixture.option_none")]
    public static Option<int> Find(
        [SpecInput("items")] List<int> items,
        [SpecInput("target")] int target)
    {
        int index = items.IndexOf(target);
        return index < 0 ? Option<int>.None() : Option<int>.Some(index);
    }

    [SpecOperation("try_divide", Spec = "fixture.result_ok")]
    [SpecOperation("try_divide", Spec = "fixture.result_err")]
    public static Result<int, string> TryDivide(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor) =>
        divisor == 0
            ? Result<int, string>.Err("division by zero")
            : Result<int, string>.Ok(dividend / divisor);
}
