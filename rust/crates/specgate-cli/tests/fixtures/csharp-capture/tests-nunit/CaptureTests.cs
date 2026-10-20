using NUnit.Framework;

namespace SpecGate.CliCaptureFixtures.NUnitTests;

[TestFixture]
public sealed class CaptureTests
{
    [Test]
    public void AddsTwoNumbers()
    {
        Assert.That(Arithmetic.Add(2, 3), Is.EqualTo(5));
    }

    [Test]
    public async Task FetchesAResponse()
    {
        Assert.That(await Remote.Fetch("a"), Is.EqualTo("response from a"));
    }

    [TestCase(1, 2, 3)]
    [TestCase(-4, 4, 0)]
    public void AddsEveryCase(int a, int b, int expected)
    {
        Assert.That(Arithmetic.Add(a, b), Is.EqualTo(expected));
    }
}
