using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

[SpecEvent("Offset")]
public sealed class Offset
{
    [SpecEvent("dx")]
    public int Dx { get; set; }

    [SpecEvent("dy")]
    public int Dy { get; set; }
}

public static class DefaultInput
{
    [SpecOperation("scale", Spec = "fixture.default_input")]
    public static int Scale(
        [SpecInput("value")] int value,
        [SpecInput("factor")] int factor) =>
        value * factor;

    [SpecOperation("shift", Spec = "fixture.default_input")]
    public static int Shift(
        [SpecInput("base")] int @base,
        [SpecInput("by")] Offset offset) =>
        @base + offset.Dx + offset.Dy;
}
