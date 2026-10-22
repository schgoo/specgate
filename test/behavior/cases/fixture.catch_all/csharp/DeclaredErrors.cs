using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.SumTypes;

public static partial class DeclaredErrors
{
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
}
