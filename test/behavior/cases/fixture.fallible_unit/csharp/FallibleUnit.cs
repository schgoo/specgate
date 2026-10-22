using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Registry.Focused.FallibleUnit;

[SpecEvent("UnitCounter")]
public sealed class UnitCounter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    // Deliberately unscoped: an unscoped setup resolves through the operation
    // it prepares, and "advance" is declared by exactly one component.
    [SpecSetup("advance")]
    public static UnitCounter Make([SpecInput("initial")] int initial) =>
        new() { Count = initial };

    [SpecOperation("advance", Spec = "fixture.fallible_unit")]
    public void Advance() => Count++;
}

public static class FallibleUnit
{
    [SpecOperation("fallible_void", Spec = "fixture.fallible_unit")]
    [SpecException(typeof(InvalidOperationException))]
    public static void FallibleVoid([SpecInput("fail")] bool fail)
    {
        if (fail)
        {
            throw new InvalidOperationException("failed");
        }
    }

    [SpecOperation("fallible_task", Spec = "fixture.fallible_unit")]
    [SpecException(typeof(InvalidOperationException))]
    public static Task FallibleTask([SpecInput("fail")] bool fail) =>
        fail
            ? Task.FromException(new InvalidOperationException("failed"))
            : Task.CompletedTask;
}
