using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static class DeclaredErrors
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

    [SpecOperation("require_in_range", Spec = "fixture.catch_all")]
    [SpecException]
    public static int RequireInRange([SpecInput("x")] int value)
    {
        if (value < 0)
        {
            throw new InvalidOperationException("too small");
        }

        if (value > 100)
        {
            throw new FormatException("too big");
        }

        return value;
    }

    [SpecOperation("divide", Spec = "fixture.unrecoverable")]
    public static int Divide(
        [SpecInput("a")] int dividend,
        [SpecInput("b")] int divisor)
    {
        if (divisor == 0)
        {
            throw new InvalidOperationException("attempt to divide by zero");
        }

        return dividend / divisor;
    }
}
