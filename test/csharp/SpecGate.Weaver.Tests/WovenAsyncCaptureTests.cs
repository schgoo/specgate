using System;
using System.Threading;
using System.Threading.Tasks;
using SpecGate.Runtime;
using SpecGate.TestSupport;
using SpecGate.WeaverFixtures;
using Xunit;

namespace SpecGate.Weaver.Tests;

/// <summary>
/// Executes woven async fixtures. Running them proves the rewritten async stubs
/// verify and that the observer completes each span only once its task settles.
/// </summary>
[Collection(WovenCaptureSerialization.Name)]
public sealed class WovenAsyncCaptureTests
{
    [Fact]
    public async Task RecordsTheAwaitedValueOfATask()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");

        int result = await AsyncOperations.DoubleAsync(4);

        Assert.Equal(8, result);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("double", span.OperationName);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(Value.Integer(4), span.Inputs["value"]);
        Assert.Equal(Value.Integer(8), span.Result());
    }

    [Fact]
    public async Task RecordsNoResultForANonGenericTask()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");

        await AsyncOperations.TouchAsync(1);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Null(span.OperationCompletion);
    }

    [Fact]
    public async Task RecordsTheAwaitedValueOfAValueTask()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");

        int result = await AsyncOperations.DoubleValueAsync(5);

        Assert.Equal(10, result);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(Value.Integer(10), span.Result());
    }

    [Fact]
    public async Task RecordsANonGenericValueTask()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");

        await AsyncOperations.TouchValueAsync(1);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal("touch_value", span.OperationName);
        Assert.Equal(SpanStatus.Ok, span.Status);
    }

    [Fact]
    public async Task RecordsAnAsyncFailureAndRethrowsIt()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");

        await Assert.ThrowsAsync<InvalidOperationException>(() => AsyncOperations.FailAsync(3));

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Error, span.Status);
    }

    [Fact]
    public async Task RecordsCancellationAsAbandonedWithoutFailingTheCaller()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");
        using CancellationTokenSource cancel = new();
        AsyncOperations.Gate = cancel;

        Task<int> pending = AsyncOperations.WaitAsync(1);
        await cancel.CancelAsync();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => pending);

        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Unset, span.Status);
    }

    [Fact]
    public async Task NestsAnAwaitedOperationUnderItsCaller()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");

        int result = await AsyncOperations.OuterAsync(2);

        Assert.Equal(5, result);
        OperationSpan[] spans = [.. session.Finish().Operations];
        OperationSpan outer = Assert.Single(spans, s => s.OperationName == "outer");
        OperationSpan inner = Assert.Single(spans, s => s.OperationName == "double");
        Assert.Equal(CaptureScenario.ScenarioSpanId, outer.ParentSpanId);
        Assert.Equal(outer.SpanId, inner.ParentSpanId);
    }
}
