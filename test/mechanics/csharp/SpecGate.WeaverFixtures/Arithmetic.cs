using SpecGate.Annotations;

namespace SpecGate.WeaverFixtures;

/// <summary>
/// Fixtures whose compiled IL the weaver instruments. Each method is written so
/// that an assertion about the recorded span checks a distinct weaving mechanic:
/// branching returns, existing exception handlers, or the implicit instance argument.
/// </summary>
public static class Arithmetic
{
    [SpecOperation("context_add", Spec = "weaver.basic")]
    public static int AddWithContext(int before,
        [SpecInput("before", Ignore = true)] Action context, int after)
    {
        context();
        return before + after;
    }

    [SpecOperation("context_only", Spec = "weaver.basic")]
    public static void ContextOnly([SpecInput("context", Ignore = true)] Action context) => context();

    [SpecOperation("catch_all_option", Spec = "weaver.exception_wrappers"), SpecException]
    public static Option<int> CatchAllOption(int state) => SelectOption(state);

    [SpecOperation("declared_option", Spec = "weaver.exception_wrappers")]
    [SpecException(typeof(InvalidOperationException))]
    public static Option<int> DeclaredOption([SpecInput("state")] int state) => SelectOption(state);

    [SpecOperation("declared_result", Spec = "weaver.exception_wrappers")]
    [SpecException(typeof(InvalidOperationException))]
    public static Result<int, int> DeclaredResult([SpecInput("state")] int state) => SelectResult(state);

    [SpecOperation("declared_scalar", Spec = "weaver.exception_wrappers")]
    [SpecException(typeof(InvalidOperationException))]
    public static int DeclaredScalar([SpecInput("state")] int state)
    {
        ThrowIfRequested(state);
        return state;
    }

    [SpecOperation("declared_unit", Spec = "weaver.exception_wrappers")]
    [SpecException(typeof(InvalidOperationException))]
    public static void DeclaredUnit([SpecInput("state")] int state) => ThrowIfRequested(state);

    internal static Option<int> SelectOption(int state)
    {
        ThrowIfRequested(state);
        return state == 0 ? Option<int>.None() : Option<int>.Some(state);
    }

    internal static Result<int, int> SelectResult(int state)
    {
        ThrowIfRequested(state);
        return state == 0 ? Result<int, int>.Err(17) : Result<int, int>.Ok(state);
    }

    internal static void ThrowIfRequested(int state)
    {
        if (state == -1)
        {
            throw new InvalidOperationException("declared");
        }
        if (state == -2)
        {
            throw new ArgumentException("undeclared");
        }
    }

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

    public abstract class ContextRoot
    {
        public abstract int Read(Action context, int value);
    }

    public abstract class ContextMiddle : ContextRoot
    {
        public override int Read([SpecInput("context", Ignore = true)] Action context, int value) => value;
    }

    public sealed class ContextLeaf : ContextMiddle
    {
        [SpecOperation("inherited_context", Spec = "weaver.basic")]
        public override int Read(Action context, int value)
        {
            context();
            return value;
        }
    }
}
