using SpecGate.Annotations;

namespace SpecGate.CliCaptureFixtures;

public static class Arithmetic
{
    [SpecOperation("add", Spec = "fixture.cli_capture.add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}

public static class Remote
{
    [SpecOperation("fetch", Spec = "fixture.cli_capture.fetch")]
    public static async Task<string> Fetch([SpecInput("url")] string url) =>
        await Task.FromResult($"response from {url}");
}
