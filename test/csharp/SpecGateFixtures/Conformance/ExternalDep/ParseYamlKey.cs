using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.ExternalDep;

/// <summary>
/// String-processing fixture: <c>parse_yaml_key</c> extracts the value of a
/// named key from a newline-delimited <c>key: value</c> document.
/// </summary>
public static class ParseYamlKeyOps
{
    /// <summary>Looks up <paramref name="key"/> in a simple YAML-like document.</summary>
    /// <param name="input">The document text, one <c>key: value</c> pair per line (spec input <c>input</c>).</param>
    /// <param name="key">The key whose value to return (spec input <c>key</c>).</param>
    /// <returns>The trimmed value for <paramref name="key"/>, or the string <c>"null"</c> if absent.</returns>
    [SpecOperation("parse_yaml_key")]
    public static string ParseYamlKey([SpecInput("input")] string input, [SpecInput("key")] string key)
    {
        foreach (var line in input.Split('\n'))
        {
            var parts = line.Split(':', 2);
            if (parts.Length == 2 && parts[0].Trim() == key)
                return parts[1].Trim();
        }
        return "null";
    }
}
