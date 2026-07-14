using SpecGate.Annotations;

namespace SpecGateFixtures;

public static class AlphaBeta
{
    [SpecOperation("alpha")]
    public static int Alpha([SpecInput("x")] int x) => x + 1;

    [SpecOperation("beta")]
    public static int Beta([SpecInput("x")] int x) => x * 2;
}
