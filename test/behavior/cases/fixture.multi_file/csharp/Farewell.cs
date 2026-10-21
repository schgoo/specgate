using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.MultiFile;

public static class Farewell
{
    [SpecOperation("farewell", Spec = "fixture.multi_file")]
    public static string Execute([SpecInput("name")] string name) => $"Goodbye, {name}!";
}
