using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class OptionResult
{
    [SpecOperation("try_divide", Spec = "fixture.result_ok")]
    public static Result<int, string> TryDivideOk(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor) =>
        Divide(dividend, divisor);

    private static Result<int, string> Divide(int dividend, int divisor) =>
        divisor == 0
            ? Result<int, string>.Err("division by zero")
            : Result<int, string>.Ok(dividend / divisor);
}
