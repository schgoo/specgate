using SpecGate.Annotations;

namespace SpecGateFixtures;

public static class ParseYamlKeyOps
{
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
