using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Registry.Components;

[SpecEvent("Widget")]
public sealed class Widget
{
    [SpecEvent("id")]
    public int Id { get; set; }

    [SpecEvent("label")]
    public string Label { get; set; } = string.Empty;
}

public static partial class ComponentOwnership
{
    [SpecOperation("make_widget", Spec = "comp.core")]
    public static Widget MakeWidget() => new() { Id = 1, Label = "widget" };
}
