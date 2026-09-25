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

public static class ProfileOperations
{
    [SpecOperation("root", Spec = "fixture.cli.profile_root")]
    public static int Root([SpecInput("value")] int value) => value + 3;

    [SpecOperation("bridge", Spec = "fixture.cli.profile_bridge")]
    public static int Bridge([SpecInput("value")] int value) => value + 2;

    [SpecOperation("leaf", Spec = "fixture.cli.profile_leaf")]
    public static int Leaf([SpecInput("value")] int value) => value + 1;
}
