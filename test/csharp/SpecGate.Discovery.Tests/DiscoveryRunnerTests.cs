using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;
using SpecGate.Annotations;
using Xunit;

namespace SpecGate.Discovery.Tests;

/// <summary>
/// Annotated declarations reflected over by <see cref="DiscoveryRunnerTests"/>.
/// They live in the test assembly so discovery runs against real metadata.
/// </summary>
public static class Fixtures
{
    [SpecEvent]
    public enum Phase
    {
        Later = 7,
        Earlier = 1,
    }

    [SpecEvent]
    public sealed class Reading
    {
        [SpecEvent]
        public Phase Phase { get; init; }

        [SpecEvent("renamed")]
        public int Raw { get; init; }

        public int Ignored { get; init; }
    }

    [SpecOperation("record_reading", Spec = "fixtures.alpha")]
    public static Reading RecordReading([SpecInput("label")] int count) => new() { Raw = count };

    [SpecOperation("beta_only", Spec = "fixtures.beta")]
    public static void BetaOnly()
    {
    }

    [SpecSetup("record_reading")]
    public static Reading PrepareReading() => new();
}

public class DiscoveryRunnerTests
{
    [Fact]
    public void TooFewArgumentsAreRejected()
    {
        RecordingSystem system = new();

        Assert.Equal(2, DiscoveryRunner.Run(["out", "assembly.dll"], system.Adapter));
        Assert.Empty(system.Files);
        Assert.Empty(system.Directories);
    }

    [Fact]
    public void ComponentInventoryIsDistinctAndOrdered()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin"], system.Adapter));

        Assert.Equal(["out"], system.Directories);
        string[] inventory = JsonSerializer.Deserialize<string[]>(system.Files[Path.Combine("out", "components.json")])!;
        Assert.Equal(["fixtures.alpha", "fixtures.beta"], inventory);
    }

    [Fact]
    public void RequestedComponentsArePublishedByPosition()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.beta", "fixtures.alpha"], system.Adapter));

        JsonElement beta = Document(system, 0);
        JsonElement alpha = Document(system, 1);
        Assert.Equal("beta_only", Assert.Single(beta.GetProperty("operations").EnumerateArray()).GetProperty("name").GetString());
        Assert.Equal(
            ["record_reading", "record_reading"],
            alpha.GetProperty("operations").EnumerateArray().Select(operation => operation.GetProperty("name").GetString()));
    }

    [Fact]
    public void UnscopedSetupStaysWithTheComponentDeclaringItsOperation()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.beta"], system.Adapter));

        Assert.DoesNotContain(
            Document(system, 0).GetProperty("operations").EnumerateArray(),
            operation => operation.GetProperty("is_setup").GetBoolean());
    }

    [Fact]
    public void OperationPublishesSemanticAndRawMetadata()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.alpha"], system.Adapter));

        JsonElement operation = Document(system, 0)
            .GetProperty("operations")
            .EnumerateArray()
            .Single(entry => !entry.GetProperty("is_setup").GetBoolean());

        Assert.False(operation.GetProperty("is_async").GetBoolean());
        Assert.True(operation.GetProperty("is_public").GetBoolean());
        Assert.True(operation.GetProperty("cs_is_static").GetBoolean());
        Assert.Equal("Reading", operation.GetProperty("return_type").GetString());
        Assert.Equal("SpecGate.Discovery.Tests.Fixtures", operation.GetProperty("cs_class").GetString());
        Assert.Equal(["label", "i32"], operation.GetProperty("params")[0].EnumerateArray().Select(entry => entry.GetString()));
        Assert.Equal(["label", "int"], operation.GetProperty("cs_params")[0].EnumerateArray().Select(entry => entry.GetString()));
    }

    /// <summary>
    /// Variants follow declaration order, not the numeric order
    /// <c>Enum.GetNames</c> imposes, and a renamed member publishes its
    /// <c>SpecEvent</c> name.
    /// </summary>
    [Fact]
    public void ReferencedTypesArePublishedInDeclarationOrder()
    {
        RecordingSystem system = new();

        Assert.Equal(0, DiscoveryRunner.Run(["out", "assembly.dll", "bin", "fixtures.alpha"], system.Adapter));

        JsonElement[] types = [.. Document(system, 0).GetProperty("types").EnumerateArray()];
        JsonElement reading = types.Single(type => type.GetProperty("name").GetString() == "Reading");
        Assert.Equal(
            ["Phase", "renamed"],
            reading.GetProperty("fields").EnumerateArray().Select(field => field[0].GetString()));

        JsonElement phase = types.Single(type => type.GetProperty("name").GetString() == "Phase");
        Assert.Equal("enum", phase.GetProperty("kind").GetString());
        Assert.Equal(
            ["Later", "Earlier"],
            phase.GetProperty("variants").EnumerateArray().Select(variant => variant.GetProperty("name").GetString()));
    }

    private static JsonElement Document(RecordingSystem system, int index) =>
        JsonDocument.Parse(system.Files[Path.Combine("out", index + ".json")]).RootElement;
}

/// <summary>
/// A <see cref="RunnerSystem"/> that reflects over the test assembly and keeps
/// every written document in memory.
/// </summary>
internal sealed class RecordingSystem
{
    private static readonly Assembly Target = typeof(Fixtures).Assembly;

    internal Dictionary<string, string> Files { get; } = [];

    internal List<string> Directories { get; } = [];

    internal RunnerSystem Adapter { get; }

    internal RecordingSystem() =>
        Adapter = new RunnerSystem(
            new FileSystemServices(
                _ => false,
                Directories.Add,
                (path, contents) => Files[path] = contents),
            new AssemblyLoadingServices(_ => Target, (_, _) => Target),
            new DependencyResolutionServices(
                _ => new AssemblyDependencyResolver(Target.Location),
                (_, _) => null,
                _ => new Subscription()));

    private sealed class Subscription : IDisposable
    {
        public void Dispose()
        {
        }
    }
}
