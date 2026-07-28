using SpecGate.Annotations;

namespace SpecGateFixtures.Conformance.SumTypes;

/// <summary>
/// Catch-all fallible operation: <c>[SpecException]</c> with no declared types
/// means every thrown exception is the <c>Err</c> arm regardless of type. The
/// two error cases throw DIFFERENT exception types to prove the catch-all is
/// not type-filtered (there is no <c>$fault</c> path for this operation).
/// </summary>
public static class CatchAllOps
{
    /// <summary>Returns <paramref name="x"/> when within [0, 100]; throws otherwise.</summary>
    /// <param name="x">The value to validate (spec input <c>x</c>).</param>
    /// <returns>The value (Ok arm); any thrown exception, of any type, becomes the Err arm.</returns>
    [SpecOperation("require_in_range", Spec = "fixture.catch_all")]
    [SpecException]
    public static int RequireInRange([SpecInput("x")] int x)
    {
        if (x < 0)
        {
            throw new InvalidOperationException("too small");
        }

        if (x > 100)
        {
            throw new FormatException("too big");
        }

        return x;
    }
}
