using System.Reflection;

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
}
