using SpecGate.Annotations;

namespace SpecGate.WeaverSetupConflictFixture;

/// <summary>
/// One method annotated as both an operation and a setup. An operation is woven
/// with a scope and an exception handler and a setup is woven without either, so
/// the combined shape is undesigned and the weaver must reject it.
/// </summary>
public static class ConflictingDeclaration
{
    [SpecOperation("make", Spec = "weaver.negative")]
    [SpecSetup("make", Spec = "weaver.negative")]
    public static int Make([SpecInput("seed")] int seed) => seed;
}
