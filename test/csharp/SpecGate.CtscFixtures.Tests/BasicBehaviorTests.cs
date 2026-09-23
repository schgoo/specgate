using SpecGate.CtscFixtures.Conformance.Async;
using SpecGate.CtscFixtures.Conformance.Basic;
using SpecGate.CtscFixtures.Conformance.Dependencies;
using Xunit;
using FarewellFixture = SpecGate.CtscFixtures.Conformance.MultiFile.Farewell;
using GreetFixture = SpecGate.CtscFixtures.Conformance.MultiFile.Greet;

namespace SpecGate.CtscFixtures.Tests;

public sealed class BasicBehaviorTests
{
    [Fact]
    public void StatelessAndMultiOperationFixturesPreserveArithmetic()
    {
        Assert.Equal(5, StatelessAdd.Add(2, 3));
        Assert.Equal(-5, StatelessAdd.Add(-8, 3));
        Assert.Equal(5, MultiOperation.Alpha(4));
        Assert.Equal(8, MultiOperation.Beta(4));
    }

    [Fact]
    public void NamedInputsAndCheckpointBehaviorRemainOrdinaryMethods()
    {
        Assert.Equal(5, NamedInputs.Divide(20, 4));

        Scaler scaler = Scaler.Make(3);
        Assert.Equal(21, scaler.Scale(7));
        Assert.Equal("  HELLO  ", CheckpointInline.AfterUpper("  HELLO  "));
        Assert.Equal("HELLO", CheckpointInline.Process("  hello  "));
    }

    [Fact]
    public void MultiFileOperationsRemainIndependent()
    {
        Assert.Equal("Hello, Ada!", GreetFixture.Execute("Ada"));
        Assert.Equal("Goodbye, Ada!", FarewellFixture.Execute("Ada"));
    }

    [Fact]
    public async Task AsyncFetchRetainsTaskMetadataAndBehavior()
    {
        Assert.Equal("response from https://example.test", await AsyncFetch.Fetch("https://example.test"));
    }

    [Fact]
    public void ExternalYamlDependencyParticipatesInTheRealBuild()
    {
        Assert.Equal("SpecGate", ExternalDependency.ParseYamlKey("name: SpecGate", "name"));
        Assert.Equal("null", ExternalDependency.ParseYamlKey("name: SpecGate", "missing"));
    }
}
