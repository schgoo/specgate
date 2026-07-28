using SpecGate.Annotations;

namespace SpecGateFixtures.Conformance.SumTypes;

/// <summary>
/// Fallible operation exercising precise <c>[SpecException]</c> resolution: the
/// declared <see cref="DivideByZeroException"/> is the <c>Err</c> arm, while an
/// undeclared <see cref="InvalidOperationException"/> falls through to
/// <c>$fault</c> (the panic analog).
/// </summary>
public static class CheckedDivideOps
{
    /// <summary>Divides <paramref name="a"/> by <paramref name="b"/>.</summary>
    /// <param name="a">The dividend (spec input <c>a</c>).</param>
    /// <param name="b">The divisor (spec input <c>b</c>).</param>
    /// <returns>
    /// The quotient (Ok arm). Throws the declared <see cref="DivideByZeroException"/>
    /// for the Err arm; an undeclared throw becomes <c>$fault</c>.
    /// </returns>
    [SpecOperation("checked_divide", Spec = "fixture.checked_divide")]
    [SpecException(typeof(DivideByZeroException))]
    public static int CheckedDivide([SpecInput("a")] int a, [SpecInput("b")] int b)
    {
        if (b == 0)
        {
            throw new DivideByZeroException("division by zero");
        }

        if (b < 0)
        {
            throw new InvalidOperationException("negative divisor");
        }

        return a / b;
    }
}
