using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.SharedSetup;

[SpecEvent("BoxVal")]
public sealed class BoxVal
{
    [SpecEvent("value")]
    public int Value { get; set; }
}

public static class Combine
{
    [SpecSetup("combine", Fills = "left", Spec = "fixture.shared_setup")]
    [SpecSetup("combine", Fills = "right", Spec = "fixture.shared_setup")]
    public static BoxVal MakeBox() => new() { Value = 2 };

    [SpecOperation("combine", Spec = "fixture.shared_setup")]
    public static int Two(
        [SpecInput("left")] BoxVal left,
        [SpecInput("right")] BoxVal right) =>
        left.Value + right.Value;

    [SpecSetup("combine_three", Fills = "a", Spec = "fixture.shared_setup")]
    [SpecSetup("combine_three", Fills = "b", Spec = "fixture.shared_setup")]
    [SpecSetup("combine_three", Fills = "c", Spec = "fixture.shared_setup")]
    public static BoxVal MakeUnit() => new() { Value = 1 };

    [SpecOperation("combine_three", Spec = "fixture.shared_setup")]
    public static int Three(
        [SpecInput("a")] BoxVal a,
        [SpecInput("b")] BoxVal b,
        [SpecInput("c")] BoxVal c) =>
        a.Value + b.Value + c.Value;
}
