using SpecGate.Annotations;
using YamlDotNet.Serialization;

namespace SpecGateFixtures.Conformance.ExternalDep;

/// <summary>
/// External-dependency fixture: <c>parse_yaml_key</c> extracts the value of a
/// named key from a YAML document, using the YamlDotNet NuGet package (the C#
/// twin of the Rust cross_dep fixture's serde_yaml). Because it depends on an
/// external package, it compiles only when the harness builds the fixture's
/// real assembly — not a source-globbed surrogate.
/// </summary>
public static class ParseYamlKeyOps
{
    /// <summary>Looks up <paramref name="key"/> in a YAML document via YamlDotNet.</summary>
    /// <param name="input">The YAML document text (spec input <c>input</c>).</param>
    /// <param name="key">The key whose value to return (spec input <c>key</c>).</param>
    /// <returns>The value for <paramref name="key"/>, or the string <c>"null"</c> if absent.</returns>
    [SpecOperation("parse_yaml_key", Spec = "fixture.cross_dep")]
    public static string ParseYamlKey([SpecInput("input")] string input, [SpecInput("key")] string key)
    {
        var deserializer = new DeserializerBuilder().Build();
        var doc = deserializer.Deserialize<Dictionary<string, string>>(input);
        return doc is not null && doc.TryGetValue(key, out var value) ? value : "null";
    }
}
