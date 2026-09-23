using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Basic;

public static class CheckpointInline
{
    [SpecOperation("after_upper", Spec = "fixture.checkpoint_inline")]
    public static string AfterUpper([SpecInput("value")] string value) => value;

    [SpecOperation("process", Spec = "fixture.checkpoint_inline")]
    public static string Process([SpecInput("data")] string data)
    {
        string upper = data.ToUpperInvariant();
        _ = AfterUpper(upper);
        return upper.Trim();
    }
}
