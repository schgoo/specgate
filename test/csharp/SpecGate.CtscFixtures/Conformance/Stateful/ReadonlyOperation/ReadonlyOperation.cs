using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Stateful.ReadonlyOperation;

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("get_count", Spec = "fixture.readonly_operation")]
    public static Counter Make() => new() { Count = 42 };

    [SpecOperation("get_count", Spec = "fixture.readonly_operation")]
    public int GetCount() => Count;
}
