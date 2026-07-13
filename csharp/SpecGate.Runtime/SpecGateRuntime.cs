using System;
using System.Collections.Generic;

namespace SpecGate.Runtime;

public static class SpecGateRuntime
{
    [ThreadStatic]
    private static List<string>? _events;

    private static List<string> Events => _events ??= new List<string>();

    public static void Reset() => Events.Clear();

    public static void EmitRun(string operation)
    {
        Events.Add("{\"kind\":\"Run\",\"operation\":" + QuoteJson(operation) + "}");
    }

    public static void EmitEvent(string name, string value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + QuoteJson(value) + "}");
    }

    public static void EmitEvent(string name, int value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + value.ToString(System.Globalization.CultureInfo.InvariantCulture) + "}");
    }

    public static void EmitEvent(string name, bool value)
    {
        Events.Add("{\"kind\":\"Event\",\"name\":" + QuoteJson(name) + ",\"value\":" + (value ? "true" : "false") + "}");
    }

    public static string GetTracesJson()
    {
        var sb = new System.Text.StringBuilder("[");
        for (int i = 0; i < Events.Count; i++)
        {
            if (i > 0) sb.Append(',');
            sb.Append(Events[i]);
        }
        sb.Append(']');
        return sb.ToString();
    }

    private static string QuoteJson(string s)
    {
        var sb = new System.Text.StringBuilder("\"");
        foreach (char c in s)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                case '\t': sb.Append("\\t"); break;
                default:
                    if (c < 0x20) sb.Append($"\\u{(int)c:x4}");
                    else sb.Append(c);
                    break;
            }
        }
        sb.Append('"');
        return sb.ToString();
    }
}