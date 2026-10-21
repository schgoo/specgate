using System.Runtime.CompilerServices;
using SpecGate.TestSupport;
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
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        OperationScope outer = SpecGateRuntime.EnterOperation(Boundaries.Of("Fetch"), "fetch", "fixture.async");
        int enteredOn = Environment.CurrentManagedThreadId;

        await DedicatedThreadHop.Instance;

        Assert.NotEqual(enteredOn, Environment.CurrentManagedThreadId);
        Assert.Same(outer, SpecGateRuntime.Current);

        SpecGateRuntime.ExitOk(outer, "response");

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("fetch", span.OperationName);
        Assert.Equal(SpanStatus.Ok, span.Status);
    }

    [Fact]
    public async Task AnOperationStartedAfterAThreadHopStillNestsUnderItsCaller()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        OperationScope outer = SpecGateRuntime.EnterOperation(Boundaries.Of("Fetch"), "fetch", "fixture.async");
        int enteredOn = Environment.CurrentManagedThreadId;

        await DedicatedThreadHop.Instance;
        Assert.NotEqual(enteredOn, Environment.CurrentManagedThreadId);

        OperationScope inner = SpecGateRuntime.EnterOperation(Boundaries.Of("Fetch"), "decode", "fixture.async");
        SpecGateRuntime.ExitOk(inner, "decoded");
        SpecGateRuntime.ExitOk(outer, "response");

        Capture capture = session.Finish();
        OperationSpan innerSpan = capture.Operation("decode");
        OperationSpan outerSpan = capture.Operation("fetch");
        Assert.Equal(outerSpan.SpanId, innerSpan.ParentSpanId);
    }

    [Fact]
    public async Task ConcurrentOperationsDoNotSeeEachOthersContext()
    {
        using CaptureSession session = CaptureScenario.Begin("fixture.async");

        await Task.WhenAll(
            RunIsolatedOperationAsync("alpha"),
            RunIsolatedOperationAsync("beta"));

        Assert.Null(SpecGateRuntime.Current);
        Capture capture = session.Finish();
        Assert.Equal(2, capture.Operations.Count);
        Assert.All(capture.Operations, span => Assert.Equal(CaptureScenario.ScenarioSpanId, span.ParentSpanId));
    }

    private static async Task RunIsolatedOperationAsync(string operation)
    {
        await Task.Yield();

        OperationScope scope = SpecGateRuntime.EnterOperation(Boundaries.Of("Fetch"), operation, "fixture.async");
        await Task.Yield();

        Assert.Same(scope, SpecGateRuntime.Current);
        SpecGateRuntime.ExitOk(scope, operation);
    }

    /// <summary>
    /// Awaitable that always resumes its continuation on a brand-new dedicated
    /// thread, so the resumed managed thread id is guaranteed to differ from the
    /// awaiting one. Completing a task from another thread is not enough: the
    /// thread pool may hand the continuation straight back to the thread that
    /// just went idle, which it reliably does on two-core Linux CI runners.
    /// </summary>
    private sealed class DedicatedThreadHop : INotifyCompletion
    {
        public static DedicatedThreadHop Instance { get; } = new();

        public bool IsCompleted => false;

        public DedicatedThreadHop GetAwaiter() => this;

        public void OnCompleted(Action continuation)
        {
            Thread worker = new(() => continuation()) { IsBackground = true };
            worker.Start();
        }

        public void GetResult()
        {
        }
    }
}
