using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class OptionResult
{
    [SpecOperation("try_divide", Spec = "fixture.result_err")]
    public static Result<int, string> TryDivideErr(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor) =>
        Divide(dividend, divisor);
}
