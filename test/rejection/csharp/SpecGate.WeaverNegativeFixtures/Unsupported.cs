using System;
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
    [SpecOperation("span_context", Spec = "weaver.negative")]
    public static int SpanContext([SpecInput("context", Ignore = true)] Span<int> context) => 0;

    [SpecSetup("span_context", Spec = "weaver.negative")]
    public static int SpanSetup([SpecInput("context", Ignore = true)] Span<int> context) => 0;

    [SpecOperation("generic_context", Spec = "weaver.negative", TypeArguments = [typeof(int)])]
    public static int GenericContext<T>([SpecInput("context", Ignore = true)] T context)
        where T : allows ref struct => 0;

    // An async void method has no awaitable for the caller, so completion cannot
    // be observed and the weaver must refuse it.
    [SpecOperation("fire_and_forget", Spec = "weaver.negative")]
    public static async void FireAndForget([SpecInput("a")] int a)
    {
        await Task.Yield();
    }
}
