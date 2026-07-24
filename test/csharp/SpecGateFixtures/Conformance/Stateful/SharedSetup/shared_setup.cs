using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.SharedSetup;

/// <summary>A boxed integer value used as a shared setup-built operation parameter.</summary>
[SpecEvent]
public class BoxVal
{
    /// <summary>The boxed value.</summary>
    [SpecEvent("value")]
    public int Value { get; set; }
}

/// <summary>
/// Shared-setup scenarios: one setup (stacked with multiple
/// <see cref="SpecSetupAttribute.Fills"/> pins) builds several parameters of
/// the same type for a single operation.
/// </summary>
public static class SharedSetupOps
{
    /// <summary>Builds a boxed value for the <c>left</c> or <c>right</c> parameter of <c>combine</c>.</summary>
    /// <param name="start">The initial boxed value (spec input <c>start</c>).</param>
    /// <returns>A new <see cref="BoxVal"/> holding <paramref name="start"/>.</returns>
    [SpecSetup("combine", Fills = "left", Spec = "fixture.shared_setup")]
    [SpecSetup("combine", Fills = "right", Spec = "fixture.shared_setup")]
    public static BoxVal MakeBox([SpecInput("start")] int start) => new() { Value = start };

    /// <summary>Sums the values of two boxed inputs.</summary>
    /// <param name="left">The first boxed value.</param>
    /// <param name="right">The second boxed value.</param>
    /// <returns>The sum of the two boxed values.</returns>
    [SpecOperation("combine", Spec = "fixture.shared_setup")]
    public static int Combine(BoxVal left, BoxVal right) => left.Value + right.Value;

    /// <summary>Builds a unit boxed value (1) for each parameter of <c>combine_three</c>.</summary>
    /// <returns>A new <see cref="BoxVal"/> holding 1.</returns>
    [SpecSetup("combine_three", Fills = "a", Spec = "fixture.shared_setup")]
    [SpecSetup("combine_three", Fills = "b", Spec = "fixture.shared_setup")]
    [SpecSetup("combine_three", Fills = "c", Spec = "fixture.shared_setup")]
    public static BoxVal MakeUnit() => new() { Value = 1 };

    /// <summary>Sums the values of three boxed inputs.</summary>
    /// <param name="a">The first boxed value.</param>
    /// <param name="b">The second boxed value.</param>
    /// <param name="c">The third boxed value.</param>
    /// <returns>The sum of the three boxed values.</returns>
    [SpecOperation("combine_three", Spec = "fixture.shared_setup")]
    public static int CombineThree(BoxVal a, BoxVal b, BoxVal c) => a.Value + b.Value + c.Value;
}
