using SpecGate.Annotations;
namespace SpecGateFixtures.Conformance.Stateful.SetupWithParams;

/// <summary>
/// State machine whose setup takes a parameter, verifying that setup inputs
/// configure the initial state (here, the counter's starting value).
/// </summary>
public class Counter
{
    /// <summary>The current count; each assignment is captured as a state mutation.</summary>
    [SpecEvent("count")]
    public int Count { get; set; }

    /// <summary>Builds a counter starting at <paramref name="initial"/>.</summary>
    /// <param name="initial">The starting count (spec input <c>initial</c>).</param>
    /// <returns>A new <see cref="Counter"/> with <see cref="Count"/> = <paramref name="initial"/>.</returns>
    [SpecSetup("increment", Spec = "fixture.setup_with_params")]
    public static Counter Make([SpecInput("initial")] int initial) => new() { Count = initial };

    /// <summary>Increments the counter by one.</summary>
    [SpecOperation("increment", Spec = "fixture.setup_with_params")]
    public void Increment() => Count += 1;
}
