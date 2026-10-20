using SpecGate.Annotations;

namespace SpecGate.WeaverFixtures;

/// <summary>
/// Fixtures whose compiled IL the weaver instruments. Each method is written so
/// that an assertion about the recorded span is also an assertion about one
/// weaving behaviour: void returns, exceptional exits, branching bodies, and nesting.
/// </summary>
public static class Arithmetic
{
    [SpecOperation("add", Spec = "weaver.basic")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;

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

    [SpecOperation("divide", Spec = "weaver.basic")]
    public static int Divide([SpecInput("numerator")] int numerator, [SpecInput("denominator")] int denominator)
        => numerator / denominator;

    [SpecOperation("guard", Spec = "weaver.basic")]
    public static void Guard([SpecInput("value")] int value)
    {
        if (value < 0)
        {
            throw new ArgumentOutOfRangeException(nameof(value), "value must not be negative");
        }
    }

    [SpecOperation("sum_all", Spec = "weaver.basic")]
    public static int SumAll([SpecInput("values")] int[] values)
    {
        int total = 0;
        foreach (int value in values)
        {
            // Nested instrumented call, so the inner span records this as its parent.
            total = Add(total, value);
        }

        return total;
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
