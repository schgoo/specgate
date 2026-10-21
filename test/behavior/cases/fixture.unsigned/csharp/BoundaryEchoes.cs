using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Basic;

public static class BoundaryEchoes
{
    [SpecOperation("echo_u32", Spec = "fixture.unsigned")]
    public static uint EchoU32([SpecInput("value")] uint value) => value;

    [SpecOperation("echo_u64", Spec = "fixture.unsigned")]
    public static ulong EchoU64([SpecInput("value")] ulong value) => value;
}
