using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

[SpecEvent("ScalarValue")]
public abstract class ScalarValue;

[SpecEvent("Integer")]
public sealed class IntegerValue : ScalarValue
{
    [SpecEvent("value")]
    public long Value { get; set; }
}

[SpecEvent("Boolean")]
public sealed class BooleanValue : ScalarValue
{
    [SpecEvent("value")]
    public bool Value { get; set; }
}

public static class ScalarTypes
{
    [SpecOperation("classify", Spec = "fixture.scalar_types")]
    public static ScalarValue Classify(
        [SpecInput("id")] long id,
        [SpecInput("active")] bool active) =>
        active ? new IntegerValue { Value = id } : new BooleanValue();
}
