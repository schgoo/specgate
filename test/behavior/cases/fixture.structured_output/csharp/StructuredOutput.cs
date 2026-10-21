using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Structured;

[SpecEvent("EntityType")]
public sealed class EntityType
{
    [SpecEvent("entity_name")]
    public string Name { get; set; } = string.Empty;

    [SpecEvent("key_properties")]
    public List<string> KeyProperties { get; set; } = [];

    [SpecEvent("structural_properties")]
    public List<string> StructuralProperties { get; set; } = [];
}

public static class StructuredOutput
{
    [SpecOperation("resolve_entity", Spec = "fixture.structured_output")]
    public static EntityType ResolveEntity() =>
        new()
        {
            Name = "Customer",
            KeyProperties = ["ID"],
            StructuralProperties = ["ID", "Name", "Email"],
        };
}
