namespace SpecGate.CliCaptureFixtures.MSTestTests;

[TestClass]
public sealed class CaptureTests
{
    [TestMethod]
    public void AddsTwoNumbers()
    {
        Assert.AreEqual(5, Arithmetic.Add(2, 3));
    }

    [TestMethod]
    public async Task FetchesAResponse()
    {
        Assert.AreEqual("response from a", await Remote.Fetch("a"));
    }

    [TestMethod]
    [DataRow(1, 2, 3)]
    [DataRow(-4, 4, 0)]
    public void AddsEveryCase(int a, int b, int expected)
    {
        Assert.AreEqual(expected, Arithmetic.Add(a, b));
    }
}
