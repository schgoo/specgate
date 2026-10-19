using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static class OptionResult
{
    [SpecOperation("find", Spec = "fixture.option_some")]
    public static Option<int> FindSome(
        [SpecInput("items")] List<int> items,
        [SpecInput("target")] int target) =>
        Locate(items, target);

    [SpecOperation("find", Spec = "fixture.option_none")]
    public static Option<int> FindNone(
        [SpecInput("items")] List<int> items,
        [SpecInput("target")] int target) =>
        Locate(items, target);

    [SpecOperation("try_divide", Spec = "fixture.result_ok")]
    public static Result<int, string> TryDivideOk(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor) =>
        Divide(dividend, divisor);

    [SpecOperation("try_divide", Spec = "fixture.result_err")]
    public static Result<int, string> TryDivideErr(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor) =>
        Divide(dividend, divisor);

    private static Option<int> Locate(List<int> items, int target)
    {
        int index = items.IndexOf(target);
        return index < 0 ? Option<int>.None() : Option<int>.Some(index);
    }

    private static Result<int, string> Divide(int dividend, int divisor) =>
        divisor == 0
            ? Result<int, string>.Err("division by zero")
            : Result<int, string>.Ok(dividend / divisor);
}
