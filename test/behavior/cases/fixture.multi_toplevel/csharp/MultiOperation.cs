using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Basic;

public static class MultiOperation
{
    [SpecOperation("alpha", Spec = "fixture.multi_toplevel")]
    public static int Alpha([SpecInput("x")] int x) => x + 1;

    [SpecOperation("beta", Spec = "fixture.multi_toplevel")]
    public static int Beta([SpecInput("x")] int x) => x * 2;
}
