using SpecGate.Annotations;

namespace SpecGateFixtures;

public static class GreetOps
{
    [SpecOperation("greet")]
    public static string Greet([SpecInput("name")] string name) => $"Hello, {name}!";
}
