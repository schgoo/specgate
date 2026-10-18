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

    [Theory]
    [InlineData(1, 2, 3)]
    [InlineData(-4, 4, 0)]
    public void AddsEveryCase(int a, int b, int expected)
    {
        Assert.Equal(expected, Arithmetic.Add(a, b));
    }
}
