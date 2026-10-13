using SpecGate.Annotations;

namespace SpecGate.WeaverSetupFixture;

/// <summary>
/// A receiver built by a woven setup, then operated on by a woven operation.
/// Running these at all proves the setup IL shape verifies.
/// </summary>
[SpecEvent("Counter")]
public sealed class Counter
{
    private Counter(int start) => Start = start;

    [SpecEvent("start")]
    public int Start { get; }

    [SpecSetup("increment", Spec = "weaver.setup")]
    public static Counter Make([SpecInput("start")] int start) => new(start);

    /// <summary>A setup whose body throws, so its provenance is never committed.</summary>
    [SpecSetup("broken", Spec = "weaver.setup")]
    public static Counter MakeBroken([SpecInput("start")] int start) =>
        throw new InvalidOperationException($"cannot build a counter from {start}");

    [SpecOperation("increment", Spec = "weaver.setup")]
    public int Increment([SpecInput("by")] int by) => Start + by;

    [SpecOperation("broken", Spec = "weaver.setup")]
    public int Broken([SpecInput("by")] int by) => Start + by;

    /// <summary>Builds a receiver without recording provenance, for the broken-setup fact.</summary>
    public static Counter Unrecorded(int start) => new(start);
}
