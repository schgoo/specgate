using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Basic;

public static class MultiCase
{
    [SpecOperation("add", Spec = "fixture.multi_case")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
