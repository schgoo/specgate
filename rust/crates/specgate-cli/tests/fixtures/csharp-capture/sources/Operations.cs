using SpecGate.Annotations;

namespace SpecGate.CliCaptureFixtures;

public static class Arithmetic
{
    [SpecOperation("add", Spec = "fixture.cli_capture.add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}

public static class Pipeline
{
    // Calls into another annotated component, so a capture of this one retains a nested foreign span.
    [SpecOperation("run", Spec = "fixture.cli_capture.pipeline")]
    public static int Run([SpecInput("a")] int a) => Arithmetic.Add(a, 1);
}

public static class Remote
{
    [SpecOperation("fetch", Spec = "fixture.cli_capture.fetch")]
    public static async Task<string> Fetch([SpecInput("url")] string url) =>
        await Task.FromResult($"response from {url}");
}
