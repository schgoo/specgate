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
    public async Task RecordsConcreteGenericSpecializations()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.generic");

        Assert.Equal(7, await AsyncOperations.EchoAsync(7));
        Assert.Equal("hello", await AsyncOperations.EchoAsync("hello"));
        Assert.Single(await AsyncOperations.ItemsAsync<AsyncOperations.Item>());

        OperationSpan[] spans = [.. session.Finish().Operations];
        Assert.Equal(["echo_i32", "echo_string", "items"], Array.ConvertAll(spans, span => span.OperationName));
        Assert.Equal(Value.Integer(7), spans[0].Result());
        Assert.Equal(Value.String("hello"), spans[1].Result());
        Assert.NotNull(spans[2].Result());
    }

    [Fact]
    public async Task UnrecordedGenericSpecializationsPreserveOrdinaryExecution()
    {
        Assert.Equal(1.5, await AsyncOperations.EchoAsync(1.5));
    }

    [Fact]
    public async Task CachedGenericSpecializationsRemainDistinctAcrossCaptureSessions()
    {
        for (int iteration = 0; iteration < 2; iteration++)
        {
            using CaptureSession session = CaptureScenario.Begin("weaver.generic");
            Assert.Equal("cached", await AsyncOperations.EchoAsync("cached"));
            Assert.Equal(iteration, await AsyncOperations.EchoAsync(iteration));
            OperationSpan[] spans = [.. session.Finish().Operations];
            Assert.Equal(["echo_string", "echo_i32"], Array.ConvertAll(spans, span => span.OperationName));
        }
    }

    [Fact]
    public void UndeclaredGenericSpecializationIsRejectedDuringCapture()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.generic");

        SpecGateCaptureException failure = Assert.Throws<SpecGateCaptureException>(
            () => { _ = AsyncOperations.EchoAsync(1.5); });

        Assert.Contains("declared specialization", failure.Message, StringComparison.Ordinal);
    }

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
    public async Task ForwardingTaskStaysOpenUntilItSettlesAndRecordsItsValue()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");
        AsyncOperations.Pending = new TaskCompletionSource<int>(TaskCreationOptions.RunContinuationsAsynchronously);

        Task<int> pending = AsyncOperations.Forward(1);

        Assert.False(pending.IsCompleted);
        AsyncOperations.Pending.SetResult(7);
        Assert.Equal(7, await pending);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Ok, span.Status);
        Assert.Equal(Value.Integer(7), span.Result());
    }

    [Fact]
    public async Task ForwardingTaskRecordsAFault()
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.async");
        AsyncOperations.Pending = new TaskCompletionSource<int>(TaskCreationOptions.RunContinuationsAsynchronously);

        Task pending = AsyncOperations.ForwardUnit(1);
        AsyncOperations.Pending.SetException(new InvalidOperationException("boom"));

        await Assert.ThrowsAsync<InvalidOperationException>(() => pending);
        OperationSpan span = Assert.Single(session.Finish().Operations);
        Assert.Equal(SpanStatus.Error, span.Status);
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
