using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class DeclaredErrors
{
    [SpecOperation("checked_divide", Spec = "fixture.checked_divide")]
    [SpecException(typeof(DivideByZeroException))]
    public static int CheckedDivide(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor)
    {
        if (divisor == 0)
        {
            throw new DivideByZeroException("division by zero");
        }

        if (divisor < 0)
        {
            throw new InvalidOperationException("negative divisor");
        }

        return dividend / divisor;
    }
}
