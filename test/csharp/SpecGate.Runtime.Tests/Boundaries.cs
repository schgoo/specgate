using System.Reflection;
using SpecGate.Annotations;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Stand-ins for the woven methods whose boundaries these tests drive by hand.
/// </summary>
/// <remarks>
/// Woven code passes its own method handle to the runtime, which reads input
/// names, declared types, and nullability back from it. So a test that calls the
/// entry points directly has to supply a real method, not a description of one.
/// </remarks>
internal static class Boundaries
{
    /// <summary>Gets the handle of the declaration named <paramref name="name"/>.</summary>
    internal static RuntimeMethodHandle Of(string name) =>
        typeof(Boundaries).GetMethod(name, BindingFlags.Static | BindingFlags.NonPublic)!.MethodHandle;

    internal static int Add(int left, int right) => left + right;

    internal static int Divide(int numerator, int denominator) => numerator / denominator;

    internal static string Process(string text) => text.ToUpperInvariant();

    internal static string Fetch() => "response";

    internal static void Reset()
    {
    }

    internal static Option<int> Find(int value) => Option<int>.Some(value);

    internal static Result<int, string> TryDivide(int numerator, int denominator) =>
        Result<int, string>.Ok(numerator / denominator);

    /// <summary>A declaration whose contract includes one error outcome.</summary>
    [SpecException(typeof(InvalidOperationException))]
    internal static int Checked(int value) => value;

    /// <summary>A declaration whose contract admits any exception as its error outcome.</summary>
    [SpecException]
    internal static int CatchAll(int value) => value;
}
