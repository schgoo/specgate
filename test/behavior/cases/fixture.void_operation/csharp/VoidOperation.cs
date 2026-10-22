using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.VoidOperation;

[SpecEvent("Logger")]
public sealed class Logger
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("log", Spec = "fixture.void_operation")]
    public static Logger Make() => new();

    [SpecOperation("log", Spec = "fixture.void_operation")]
    public void Log([SpecInput("msg")] string message)
    {
        ArgumentNullException.ThrowIfNull(message);
        Count++;
        _ = ObserveCount(Count);
    }

    [SpecOperation("observe_count", Spec = "fixture.void_operation")]
    public static int ObserveCount([SpecInput("count")] int count) => count;
}
