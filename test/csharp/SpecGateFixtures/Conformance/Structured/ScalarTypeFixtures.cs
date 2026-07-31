using SpecGate.Annotations;

namespace SpecGateFixtures.Conformance.Structured;

/// <summary>
/// Fixtures covering the scalar built-in types <c>i64</c>, <c>bool</c>, and the
/// universal <c>value</c>. The C# counterpart of the runtime <c>Value</c> is
/// <see cref="object"/>: a boxed <see cref="long"/> or <see cref="bool"/> is
/// marshalled to the same canonical value stream the Rust <c>Value</c> emits.
/// </summary>
public static class ScalarTypeFixtures
{
    /// <summary>
    /// Returns the id as the universal value when <paramref name="active"/>, or
    /// the boolean <c>false</c> otherwise — surfacing i64, bool, and value.
    /// </summary>
    /// <param name="id">A 64-bit integer id (spec input <c>id</c>).</param>
    /// <param name="active">Whether to echo the id (spec input <c>active</c>).</param>
    /// <returns>The boxed id when active; otherwise boxed <c>false</c>.</returns>
    [SpecOperation("classify", Spec = "fixture.scalar_types")]
    public static object Classify([SpecInput("id")] long id, [SpecInput("active")] bool active)
    {
        if (active)
        {
            return id;
        }

        return false;
    }
}
