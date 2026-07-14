using SpecGate.Annotations;

namespace SpecGateFixtures;

public static class DivideOps
{
    [SpecOperation("divide")]
    public static int Divide([SpecInput("a")] int a, [SpecInput("b")] int b)
    {
        if (b == 0) throw new System.Exception("attempt to divide by zero");
        return a / b;
    }
}
