using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Basic;

public static class NamedInputs
{
    [SpecOperation("divide", Spec = "fixture.named_inputs")]
    public static int Divide(
        [SpecInput("numerator")] int dividend,
        [SpecInput("denominator")] int divisor) =>
        dividend / divisor;
}

[SpecEvent("Scaler")]
public sealed class Scaler
{
    [SpecEvent("factor")]
    public int Factor { get; set; }

    [SpecSetup("scale", Spec = "fixture.named_inputs")]
    public static Scaler Make([SpecInput("factor")] int multiplier) =>
        new() { Factor = multiplier };

    [SpecOperation("scale", Spec = "fixture.named_inputs")]
    public int Scale([SpecInput("value")] int operand) => Factor * operand;
}
