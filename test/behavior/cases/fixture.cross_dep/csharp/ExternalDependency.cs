using SpecGate.Annotations;
using YamlDotNet.Serialization;

namespace SpecGate.CtscFixtures.Conformance.Dependencies;

public static class ExternalDependency
{
    [SpecOperation("parse_yaml_key", Spec = "fixture.cross_dep")]
    public static string ParseYamlKey(
        [SpecInput("input")] string input,
        [SpecInput("key")] string key)
    {
        IDeserializer deserializer = new DeserializerBuilder().Build();
        Dictionary<string, string>? document =
            deserializer.Deserialize<Dictionary<string, string>>(input);
        return document is not null && document.TryGetValue(key, out string? value)
            ? value
            : "null";
    }
}
