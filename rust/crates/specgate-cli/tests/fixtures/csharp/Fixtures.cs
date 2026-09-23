using SpecGate.Annotations;

namespace SpecGate.CliFixtures;

public static class ReplayOperations
{
    [SpecOperation("add", Spec = "fixture.cli.replay")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;

    [SpecOperation("echo", Spec = "fixture.cli.replay")]
    public static string Echo([SpecInput("value")] string value) => value;
}

[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.cli.setup")]
    public static Counter Make([SpecInput("initial")] int initial) =>
        new() { Count = initial };

    [SpecOperation("increment", Spec = "fixture.cli.setup")]
    public void Increment() => Count++;
}

public static class MultipleOperations
{
    [SpecOperation("used", Spec = "fixture.cli.multiple")]
    public static int Used([SpecInput("value")] int value) => value + 1;

    [SpecOperation("unexercised", Spec = "fixture.cli.multiple")]
    public static int Unexercised([SpecInput("value")] int value) => value - 1;
}
