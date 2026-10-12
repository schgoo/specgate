using Xunit;

namespace SpecGate.Runtime.Tests;

/// <summary>
/// Proves the capture context flows with the execution context rather than the
/// thread. Every assertion here fails if the active operation is held in
/// thread-local storage.
/// </summary>
[Collection(nameof(CaptureSessionSerialization))]
public sealed class LogicalContextFlowTests
{
    [Fact]
    public async Task TheActiveOperationSurvivesAContinuationOnAnotherThread()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.async");

        OperationScope outer = SpecGateRuntime.EnterOperation("fetch", "fixture.async");
        int enteredOn = Environment.CurrentManagedThreadId;

        int resumedOn = await HopToAnotherThreadAsync(enteredOn);

        Assert.NotEqual(enteredOn, resumedOn);
        Assert.Same(outer, SpecGateRuntime.Current);

        SpecGateRuntime.ExitOk(outer, "response");

        RecordedSpan span = Assert.Single(session.Spans);
        Assert.Equal("fetch", span.Operation);
        Assert.Equal(SpanStatus.Ok, span.Status);
    }

    [Fact]
    public async Task AnOperationStartedAfterAThreadHopStillNestsUnderItsCaller()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.async");

        OperationScope outer = SpecGateRuntime.EnterOperation("fetch", "fixture.async");
        int enteredOn = Environment.CurrentManagedThreadId;

        int resumedOn = await HopToAnotherThreadAsync(enteredOn);
        Assert.NotEqual(enteredOn, resumedOn);

        OperationScope inner = SpecGateRuntime.EnterOperation("decode", "fixture.async");
        SpecGateRuntime.ExitOk(inner, "decoded");
        SpecGateRuntime.ExitOk(outer, "response");

        RecordedSpan innerSpan = session.Spans.Single(span => span.Operation == "decode");
        RecordedSpan outerSpan = session.Spans.Single(span => span.Operation == "fetch");
        Assert.Equal(outerSpan.SpanId, innerSpan.ParentSpanId);
    }

    [Fact]
    public async Task ConcurrentOperationsDoNotSeeEachOthersContext()
    {
        using CaptureSession session = SpecGateRuntime.BeginSession("fixture.async");

        await Task.WhenAll(
            RunIsolatedOperationAsync("alpha"),
            RunIsolatedOperationAsync("beta"));

        Assert.Null(SpecGateRuntime.Current);
        Assert.Equal(2, session.Spans.Count);
        Assert.All(session.Spans, span => Assert.Null(span.ParentSpanId));
    }

    private static async Task RunIsolatedOperationAsync(string operation)
    {
        await Task.Yield();

        OperationScope scope = SpecGateRuntime.EnterOperation(operation, "fixture.async");
        await Task.Yield();

        Assert.Same(scope, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(scope, operation);
    }

    /// <summary>
    /// Completes a task from a freshly started thread until the awaiting code
    /// actually resumes on a different thread, then reports that thread.
    /// </summary>
    private static async Task<int> HopToAnotherThreadAsync(int startingThreadId)
    {
        for (int attempt = 0; attempt < 50; attempt++)
        {
            TaskCompletionSource<bool> signal =
                new(TaskCreationOptions.RunContinuationsAsynchronously);
            Thread signaller = new(() => signal.SetResult(true)) { IsBackground = true };
            signaller.Start();

            await signal.Task.ConfigureAwait(false);

            if (Environment.CurrentManagedThreadId != startingThreadId)
            {
                return Environment.CurrentManagedThreadId;
            }
        }

        throw new InvalidOperationException(
            "could not force a continuation onto a different thread after 50 attempts.");
    }
}
