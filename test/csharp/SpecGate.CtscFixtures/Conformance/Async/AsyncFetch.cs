using SpecGate.Annotations;

namespace SpecGate.CtscFixtures.Conformance.Async;

public static class AsyncFetch
{
    [SpecOperation("fetch", Spec = "fixture.async_fetch")]
    public static async Task<string> Fetch([SpecInput("url")] string url) =>
        await Task.FromResult($"response from {url}");
}
