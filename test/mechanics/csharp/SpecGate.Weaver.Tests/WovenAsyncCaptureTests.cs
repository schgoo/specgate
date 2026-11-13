using System;
using System.Linq;
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
    [Theory]
    [InlineData(0)]
    [InlineData(7)]
    public async Task DeclaredExceptionsPreserveTheFullAwaitedReturn(int state)
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.exception_wrappers");
        _ = await AsyncOperations.DeclaredOptionTask(state);
        _ = await AsyncOperations.DeclaredOptionValueTask(state);
        _ = await AsyncOperations.DeclaredResultTask(state);
        _ = await AsyncOperations.DeclaredResultValueTask(state);
        Assert.Equal(state, await AsyncOperations.DeclaredScalarTask(state));
        Assert.Equal(state, await AsyncOperations.DeclaredScalarValueTask(state));
        await AsyncOperations.DeclaredUnitTask(state);
        await AsyncOperations.DeclaredUnitValueTask(state);
        _ = await AsyncOperations.CatchAllOptionTask(state);
        _ = await AsyncOperations.CatchAllOptionValueTask(state);

        OperationSpan[] spans = [.. session.Finish().Operations];
        Assert.Equal(10, spans.Length);
        Assert.All(spans, span => Assert.Equal(SpanStatus.Ok, span.Status));
        Value option = state == 0
            ? Value.Tagged("None", Value.Unit)
            : Value.Tagged("Some", Value.Integer(state));
        Value result = state == 0
            ? Value.Tagged("Err", Value.Integer(17))
            : Value.Tagged("Ok", Value.Integer(state));
        Assert.Equal(option, spans[0].Result());
        Assert.Equal(option, spans[1].Result());
        Assert.Equal(result, spans[2].Result());
        Assert.Equal(result, spans[3].Result());
        Assert.Equal(Value.Integer(state), spans[4].Result());
        Assert.Equal(Value.Integer(state), spans[5].Result());
        Assert.Null(spans[6].OperationCompletion);
        Assert.Null(spans[7].OperationCompletion);
        Assert.Equal(option, spans[8].Result());
        Assert.Equal(option, spans[9].Result());
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(-2)]
    public async Task OnlyActualDeclaredAsyncThrowsUseTheSyntheticErrorChannel(int state)
    {
        using CaptureSession session = CaptureScenario.Begin("weaver.exception_wrappers");
        await Assert.ThrowsAnyAsync<Exception>(() => AsyncOperations.DeclaredOptionTask(state));
        await Assert.ThrowsAnyAsync<Exception>(async () => await AsyncOperations.DeclaredOptionValueTask(state));
        await Assert.ThrowsAnyAsync<Exception>(() => AsyncOperations.DeclaredResultTask(state));
        await Assert.ThrowsAnyAsync<Exception>(async () => await AsyncOperations.DeclaredResultValueTask(state));
        await Assert.ThrowsAnyAsync<Exception>(() => AsyncOperations.DeclaredScalarTask(state));
        await Assert.ThrowsAnyAsync<Exception>(async () => await AsyncOperations.DeclaredScalarValueTask(state));
        await Assert.ThrowsAnyAsync<Exception>(() => AsyncOperations.DeclaredUnitTask(state));
        await Assert.ThrowsAnyAsync<Exception>(async () => await AsyncOperations.DeclaredUnitValueTask(state));
        await Assert.ThrowsAnyAsync<Exception>(() => AsyncOperations.CatchAllOptionTask(state));
        await Assert.ThrowsAnyAsync<Exception>(async () => await AsyncOperations.CatchAllOptionValueTask(state));

        Capture capture = session.Finish();
        Assert.Equal(10, capture.Operations.Count);
        Assert.All(capture.Operations.Take(8), span =>
        {
            Assert.Equal(SpanStatus.Error, span.Status);
            if (state == -1)
            {
                ErrorCompletion error = Assert.IsType<ErrorCompletion>(span.OperationCompletion);
                Assert.Equal("error", error.Name);
                Assert.Equal(Value.String("declared"), error.ErrorValue);
            }
            else
            {
                Assert.IsType<FaultCompletion>(span.OperationCompletion);
            }
        });
        Assert.All(capture.Operations.Skip(8), span =>
        {
            ErrorCompletion caught = Assert.IsType<ErrorCompletion>(span.OperationCompletion);
            Assert.Equal("error", caught.Name);
            Assert.Equal(Value.String(state == -1 ? "declared" : "undeclared"), caught.ErrorValue);
        });
    }

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
