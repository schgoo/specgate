using SpecGate.Annotations;

namespace SpecGateFixtures.Conformance.Basic;

/// <summary>
/// Language-neutral input names via <c>[SpecInput(...)]</c>: the C# parameter
/// names differ from the spec input names, on a free-function operation, a
/// setup, and a method operation. The focused cross-language proof that input
/// renaming is honored identically.
/// </summary>
public static class NamedInputsOps
{
    /// <summary>Divides <paramref name="a"/> by <paramref name="b"/> with renamed inputs.</summary>
    /// <param name="a">The dividend (spec input <c>numerator</c>).</param>
    /// <param name="b">The divisor (spec input <c>denominator</c>).</param>
    /// <returns>The integer quotient.</returns>
    [SpecOperation("divide", Spec = "fixture.named_inputs")]
    public static int Divide([SpecInput("numerator")] int a, [SpecInput("denominator")] int b) => a / b;
}

/// <summary>
/// A scaler whose construction input (<c>factor</c>) is routed to the setup by
/// name, while the operation input (<c>value</c>) is routed to the method — both
/// renamed from the C# parameter identifiers.
/// </summary>
public class Scaler
{
    /// <summary>The scale factor, captured as initial state.</summary>
    [SpecEvent("factor")]
    public int Factor { get; set; }

    /// <summary>Builds a scaler with the given factor (spec input <c>factor</c>).</summary>
    /// <param name="f">The scale factor (spec input <c>factor</c>).</param>
    /// <returns>A new <see cref="Scaler"/>.</returns>
    [SpecSetup("scale", Spec = "fixture.named_inputs")]
    public static Scaler Make([SpecInput("factor")] int f) => new() { Factor = f };

    /// <summary>Scales <paramref name="v"/> by the factor.</summary>
    /// <param name="v">The value to scale (spec input <c>value</c>).</param>
    /// <returns>The product <c>factor * value</c>.</returns>
    [SpecOperation("scale", Spec = "fixture.named_inputs")]
    public int Scale([SpecInput("value")] int v) => Factor * v;
}
