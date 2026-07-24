using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.MultiFile;

/// <summary>
/// Farewell fixture used by the multi-file scenario, paired with
/// <see cref="GreetOps"/> to prove operations split across files are all
/// discovered and compiled together.
/// </summary>
public static class FarewellOps
{
    /// <summary>Builds a farewell for <paramref name="name"/>.</summary>
    /// <param name="name">The name to bid farewell (spec input <c>name</c>).</param>
    /// <returns>The string <c>"Goodbye, {name}!"</c>.</returns>
    [SpecOperation("farewell", Spec = "fixture.multi_file")]
    public static string Farewell([SpecInput("name")] string name) => $"Goodbye, {name}!";
}
