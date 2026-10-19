using Xunit;

namespace SpecGate.CliCaptureFixtures.Tests;

public sealed class CaptureTests
{
    [Fact]
    public void AddsTwoNumbers()
    {
        Assert.Equal(5, Arithmetic.Add(2, 3));
    }

    [Fact]
    public async Task FetchesAResponse()
    {
        Assert.Equal("response from a", await Remote.Fetch("a"));
    }

    [Fact]
    public void RunsThroughANestedComponent()
    {
        Assert.Equal(4, Pipeline.Run(3));
    }

    // The display name carries filter-reserved characters, which must still select exactly this case.
    [Theory]
    [InlineData("a;b&c%2C|d")]
    public void AddsAwkwardlyNamedLengths(string label)
    {
        Assert.Equal(label.Length, Arithmetic.Add(label.Length, 0));
    }

    [Theory]
    [InlineData(1, 2, 3)]
    [InlineData(-4, 4, 0)]
    public void AddsEveryCase(int a, int b, int expected)
    {
        Assert.Equal(expected, Arithmetic.Add(a, b));
    }
}
