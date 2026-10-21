using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Registry.Components;

public static partial class ComponentOwnership
{
    [SpecOperation("assemble", Spec = "comp.app")]
    public static Widget Assemble() => new() { Id = 2, Label = "assembled" };
}
