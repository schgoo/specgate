using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class DeclaredErrors
{
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
