using System.Threading.Tasks;
using SpecGate.Annotations;

namespace SpecGate.WeaverNegativeFixtures;

/// <summary>
/// Declarations the weaver must reject rather than silently mis-instrument.
/// This assembly is never woven during the build; the weaver tests weave a copy
/// of it and assert on the failure.
/// </summary>
public static class Unsupported
{
    [SpecOperation("delayed_add", Spec = "weaver.negative")]
    public static async Task<int> DelayedAddAsync([SpecInput("a")] int a, [SpecInput("b")] int b)
    {
        await Task.Yield();
        return a + b;
    }
}
