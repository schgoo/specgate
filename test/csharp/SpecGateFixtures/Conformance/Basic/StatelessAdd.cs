using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Basic;

/// <summary>
/// Stateless free-function fixture: a pure <c>add</c> operation over two
/// integer inputs. Exercises the harness's most basic scalar-return path.
/// </summary>
public static class StatelessAdd
{
    /// <summary>Adds two integers and returns their sum.</summary>
    /// <param name="a">The first addend (spec input <c>a</c>).</param>
    /// <param name="b">The second addend (spec input <c>b</c>).</param>
    /// <returns>The sum <paramref name="a"/> + <paramref name="b"/>.</returns>
    [SpecOperation("add", Spec = "fixture.stateless_add")]
    [SpecOperation("add", Spec = "fixture.multi_case")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
