using SpecGate.Annotations;

namespace SpecGateFixtures;

public static class FarewellOps
{
    [SpecOperation("farewell")]
    public static string Farewell([SpecInput("name")] string name) => $"Goodbye, {name}!";
}
