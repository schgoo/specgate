using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Basic;

public static class StatelessAdd
{
    [SpecOperation("add", Spec = "fixture.stateless_add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
