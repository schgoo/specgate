using SpecGate.Annotations;

namespace SpecGateFixtures.AsyncFetch;

/// <summary>
/// Async operation fixture — mirrors the Rust <c>fetch</c> async fn. The body
/// awaits an immediately-ready task so the <c>await</c> machinery is genuinely
/// exercised without requiring a reactor, proving the harness awaits async ops
/// and unwraps <see cref="Task{TResult}"/> for the result.
/// </summary>
public static class Fetcher
{
    /// <summary>Returns a canned response for <paramref name="url"/>.</summary>
    /// <param name="url">The URL to fetch (spec input <c>url</c>).</param>
    /// <returns>A response string for the given URL.</returns>
    [SpecOperation("fetch")]
    public static async Task<string> Fetch([SpecInput("url")] string url) =>
        await Task.FromResult($"response from {url}");
}
