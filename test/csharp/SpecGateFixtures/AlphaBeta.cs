using SpecGate.Annotations;

namespace SpecGateFixtures;

/// <summary>
/// Two unrelated operations in one fixture, used to verify that multiple
/// top-level operations coexist and are discovered independently.
/// </summary>
public static class AlphaBeta
{
    /// <summary>Increments <paramref name="x"/> by one.</summary>
    /// <param name="x">The input value (spec input <c>x</c>).</param>
    /// <returns><paramref name="x"/> + 1.</returns>
    [SpecOperation("alpha")]
    public static int Alpha([SpecInput("x")] int x) => x + 1;

    /// <summary>Doubles <paramref name="x"/>.</summary>
    /// <param name="x">The input value (spec input <c>x</c>).</param>
    /// <returns><paramref name="x"/> * 2.</returns>
    [SpecOperation("beta")]
    public static int Beta([SpecInput("x")] int x) => x * 2;
}
