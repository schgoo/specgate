using SpecGate.Annotations;
using SpecGate.Runtime;

namespace SpecGateFixtures.Conformance.Basic;

/// <summary>
/// Inline-checkpoint fixture: <c>process</c> records a named intermediate value
/// mid-operation via <see cref="SpecEvent.Record(string, object?)"/> (the C#
/// analog of Rust's <c>spec_trace!</c>), then returns the trimmed result. The
/// focused cross-language proof that both targets emit the checkpoint identically.
/// </summary>
public static class CheckpointInline
{
    /// <summary>
    /// Uppercases <paramref name="data"/>, records the pre-trim value as the
    /// <c>after_upper</c> checkpoint, then returns the trimmed uppercase string.
    /// </summary>
    /// <param name="data">The input string (spec input <c>data</c>).</param>
    /// <returns>The uppercased, trimmed string.</returns>
    [SpecOperation("process", Spec = "fixture.checkpoint_inline")]
    public static string Process([SpecInput("data")] string data)
    {
        var upper = data.ToUpperInvariant();
        SpecEvent.Record("after_upper", upper);
        return upper.Trim();
    }
}
