using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.MultiFile;

public static class Greet
{
    [SpecOperation("greet", Spec = "fixture.multi_file")]
    public static string Execute([SpecInput("name")] string name) => $"Hello, {name}!";
}
