using SpecGate.Annotations;

namespace SpecGateFixtures.MultiFile;

/// <summary>
/// Greeting fixture used by the multi-file scenario, paired with
/// <see cref="FarewellOps"/> to prove operations split across files are all
/// discovered and compiled together.
/// </summary>
public static class GreetOps
{
    /// <summary>Builds a greeting for <paramref name="name"/>.</summary>
    /// <param name="name">The name to greet (spec input <c>name</c>).</param>
    /// <returns>The string <c>"Hello, {name}!"</c>.</returns>
    [SpecOperation("greet")]
    public static string Greet([SpecInput("name")] string name) => $"Hello, {name}!";
}
