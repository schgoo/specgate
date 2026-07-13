using SpecGate.Annotations;

namespace SpecGateFixtures;

public static class StatelessAdd
{
    [SpecOperation("add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
