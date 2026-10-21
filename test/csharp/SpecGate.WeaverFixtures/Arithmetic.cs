using SpecGate.Annotations;

namespace SpecGate.WeaverFixtures;

/// <summary>
/// Fixtures whose compiled IL the weaver instruments. Each method is written so
/// that an assertion about the recorded span checks a distinct weaving mechanic:
/// branching returns, existing exception handlers, or the implicit instance argument.
/// </summary>
public static class Arithmetic
{
    [SpecOperation("describe", Spec = "weaver.basic")]
    public static string Describe([SpecInput("value")] int value)
    {
        // Two returns, so the weaver must route both through one epilogue.
        if (value < 0)
        {
            return "negative";
        }

        return value == 0 ? "zero" : "positive";
    }

    [SpecOperation("parse_or_default", Spec = "weaver.basic")]
    public static int ParseOrDefault([SpecInput("text")] string? text)
    {
        // An existing try/catch in the body, which the weaver must wrap rather than disturb.
        try
        {
            return int.Parse(text!, System.Globalization.CultureInfo.InvariantCulture);
        }
        catch (FormatException)
        {
            return 0;
        }
    }
}

/// <summary>An instance fixture, so the weaver must skip the implicit this argument.</summary>
public sealed class Accumulator
{
    private int _total;

    [SpecOperation("accumulate", Spec = "weaver.basic")]
    public int Accumulate([SpecInput("amount")] int amount)
    {
        _total += amount;
        return _total;
    }
}
