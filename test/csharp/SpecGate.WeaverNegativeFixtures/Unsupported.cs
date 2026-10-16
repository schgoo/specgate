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
    // An async void method has no awaitable for the caller, so completion cannot
    // be observed and the weaver must refuse it.
    [SpecOperation("fire_and_forget", Spec = "weaver.negative")]
    public static async void FireAndForget([SpecInput("a")] int a)
    {
        await Task.Yield();
    }
}
