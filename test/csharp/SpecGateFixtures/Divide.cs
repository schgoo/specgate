using SpecGate.Annotations;

namespace SpecGateFixtures;

/// <summary>
/// Fixture exercising fault capture: <c>divide</c> throws on a zero divisor so
/// the harness records the exception as a <c>$fault</c> trace event.
/// </summary>
public static class DivideOps
{
    /// <summary>Divides <paramref name="a"/> by <paramref name="b"/>.</summary>
    /// <param name="a">The dividend (spec input <c>a</c>).</param>
    /// <param name="b">The divisor (spec input <c>b</c>).</param>
    /// <returns>The integer quotient <paramref name="a"/> / <paramref name="b"/>.</returns>
    /// <exception cref="Exception">Thrown when <paramref name="b"/> is zero; captured as a <c>$fault</c>.</exception>
    [SpecOperation("divide")]
    public static int Divide([SpecInput("a")] int a, [SpecInput("b")] int b)
    {
        if (b == 0) throw new Exception("attempt to divide by zero");
        return a / b;
    }
}
